//! Contract tests over the wire shapes in `tests/fixtures/`: every fixture
//! must parse through the same code the CLI uses live, and must be scrubbed
//! per `tests/fixtures/README.md`. Fixtures are files, never string literals.

use serde_json::Value;
use tplc::api::errors::ERR_MFA_REQUIRED;
use tplc::api::nbu::{Family, Thing};
use tplc::api::response::ApiResponse;
use tplc::models::device_info::DeviceInfo;
use tplc::models::device_type::DeviceType;
use tplc::models::energy::{CurrentPower, DayPowerSummary, MonthPowerSummary};
use tplc::models::light_state::LightState;
use tplc::models::net_info::DeviceNetInfo;
use tplc::models::schedule::ScheduleRule;
use tplc::models::time::DeviceTime;

fn fixture(rel: &str) -> Value {
    let path = format!("{}/tests/fixtures/{rel}", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parsing {path}: {e}"))
}

/// The cloud envelope, and the double-encoded `responseData` a passthrough
/// carries, unwrapped to the `<service>.<method>` block the models read.
fn passthrough(rel: &str, service: &str, method: &str) -> Value {
    let env: ApiResponse = serde_json::from_value(fixture(rel)).expect("envelope");
    assert!(env.successful(), "{rel}: error_code {}", env.error_code);
    let data = env.result.expect("result")["responseData"]
        .as_str()
        .expect("responseData is a JSON string")
        .to_string();
    let inner: Value = serde_json::from_str(&data).expect("responseData parses");
    inner[service][method].clone()
}

fn all_fixtures() -> Vec<(String, Value)> {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_some_and(|e| e == "json") {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            out.push((name.clone(), fixture(&name)));
        }
    }
    assert!(!out.is_empty());
    out
}

#[test]
fn device_lists_parse_into_typed_devices_with_known_models() {
    for (rel, cloud) in [
        ("get_device_list_response.json", "kasa"),
        ("tapo_get_device_list_response.json", "tapo"),
    ] {
        let env: ApiResponse = serde_json::from_value(fixture(rel)).unwrap();
        assert!(env.successful());
        let list = env.result.unwrap()["deviceList"]
            .as_array()
            .cloned()
            .unwrap();
        assert!(!list.is_empty(), "{rel}");
        for d in &list {
            let info = DeviceInfo::from_json(d).unwrap_or_else(|| panic!("{rel}: {d}"));
            assert!(!info.id().is_empty());
            assert_ne!(info.alias_or_name(), "Unknown");
            let dtype = DeviceType::from_model(info.model());
            assert_ne!(dtype, DeviceType::Unknown, "{rel}: {}", info.model());
            if cloud == "tapo" {
                assert!(dtype.is_tapo(), "{}", info.model());
            }
        }
    }
}

#[test]
fn login_shapes() {
    let ok: ApiResponse = serde_json::from_value(fixture("v2_login_response.json")).unwrap();
    assert!(ok.successful());
    let r = ok.result.unwrap();
    assert!(r["token"].is_string() && r["refreshToken"].is_string());

    let mfa: ApiResponse =
        serde_json::from_value(fixture("v2_login_mfa_required_response.json")).unwrap();
    assert_eq!(mfa.error_code, ERR_MFA_REQUIRED);
    assert_eq!(mfa.result.unwrap()["mfaType"], "verifyCodeLogin");

    let status: ApiResponse =
        serde_json::from_value(fixture("v2_account_status_response.json")).unwrap();
    assert!(status.result.unwrap()["appServerUrl"].is_string());

    let svc: ApiResponse =
        serde_json::from_value(fixture("v2_get_app_service_url_response.json")).unwrap();
    let urls = &svc.result.unwrap()["serviceUrls"];
    assert!(urls[tplc::api::nbu::APP_SERVER_SERVICE_ID]
        .as_str()
        .unwrap()
        .starts_with("https://"));
}

#[test]
fn passthrough_shapes_feed_the_models() {
    let rt = passthrough(
        "get_device_emeter_realtime_response.json",
        "emeter",
        "get_realtime",
    );
    let p = CurrentPower::from_json(&rt);
    assert!(p.power_mw.is_some() && p.total_wh.is_some());

    let day = passthrough(
        "get_device_emeter_daystat_response.json",
        "emeter",
        "get_daystat",
    );
    let days: Vec<DayPowerSummary> = day["day_list"]
        .as_array()
        .unwrap()
        .iter()
        .map(DayPowerSummary::from_json)
        .collect();
    assert!(days
        .iter()
        .all(|d| d.day.is_some() && d.energy_wh.is_some()));

    let month = passthrough(
        "get_device_emeter_monthstat_response.json",
        "emeter",
        "get_monthstat",
    );
    let months: Vec<MonthPowerSummary> = month["month_list"]
        .as_array()
        .unwrap()
        .iter()
        .map(MonthPowerSummary::from_json)
        .collect();
    assert!(!months.is_empty() && months.iter().all(|m| m.month.is_some()));

    let net = passthrough("get_device_net_info_response.json", "netif", "get_stainfo");
    let n = DeviceNetInfo::from_json(&net);
    assert!(n.ssid.is_some() && n.rssi.is_some());

    let time = passthrough("get_device_time_response.json", "time", "get_time");
    let t = DeviceTime::from_json(&time);
    assert!(t.year.is_some() && t.sec.is_some());

    let rules = passthrough(
        "get_device_schedule_rules_response.json",
        "schedule",
        "get_rules",
    );
    let list = rules["rule_list"].as_array().unwrap();
    assert!(!list.is_empty());
    for r in list {
        let rule = ScheduleRule::from_json(r).expect("rule parses");
        assert!(rule.id.is_some() && rule.wday.as_ref().is_some_and(|w| w.len() == 7));
    }

    let strip = passthrough("hs300_get_sys_info_response.json", "system", "get_sysinfo");
    let children = strip["children"].as_array().unwrap();
    assert_eq!(children.len(), 6, "an HS300 has six outlets");
    let parent_id = strip["deviceId"].as_str().unwrap();
    for c in children {
        assert!(
            c["id"].as_str().unwrap().starts_with(parent_id),
            "outlet ids extend the parent id"
        );
    }

    let bulb = passthrough("kl430_get_sys_info_response.json", "system", "get_sysinfo");
    let ls = LightState::from_json(&bulb["light_state"]);
    assert!(ls.on_off.is_some() && ls.brightness.is_some());

    let plug = passthrough("hs103_get_sys_info_response.json", "system", "get_sysinfo");
    assert!(plug["relay_state"].is_number());
}

#[test]
fn nbu_shapes_parse_and_join_on_device_id() {
    let fam = fixture("nbu_families_response.json");
    let families: Vec<Family> = fam["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| serde_json::from_value(v.clone()).expect("family"))
        .collect();
    assert_eq!(fam["total"].as_u64().unwrap() as usize, families.len());
    assert!(families[0].is_default);
    assert!(families[0].rooms.len() >= 2);

    let th = fixture("nbu_things_response.json");
    let things: Vec<Thing> = th["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| serde_json::from_value(v.clone()).expect("thing"))
        .collect();
    assert_eq!(th["total"].as_u64().unwrap() as usize, things.len());
    // Every roomId names a room in the family; a null roomId is "unassigned".
    let room_ids: Vec<&str> = families
        .iter()
        .flat_map(|f| f.rooms.iter().map(|r| r.id.as_str()))
        .collect();
    let mut assigned = 0;
    for t in &things {
        assert!(t.display_name().is_some(), "nicknames decode: {t:?}");
        if let Some(r) = &t.room_id {
            assert!(room_ids.contains(&r.as_str()), "{r}");
            assigned += 1;
        }
    }
    assert!(assigned >= 1);
    // The join key: a Kasa device's thingName is its account-cloud deviceId.
    let kasa = fixture("get_device_list_response.json");
    let kasa_ids: Vec<&str> = kasa["result"]["deviceList"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["deviceId"].as_str().unwrap())
        .collect();
    assert!(things
        .iter()
        .any(|t| kasa_ids.contains(&t.thing_name.as_str())));
}

/// The scrubbing policy from `tests/fixtures/README.md`, enforced.
#[test]
fn fixtures_carry_only_dummy_identities() {
    for (name, v) in all_fixtures() {
        let mut strings = Vec::new();
        collect_strings(&v, &mut strings);
        for s in strings {
            if s.contains('@') {
                assert!(s.ends_with("@example.com"), "{name}: email `{s}`");
            }
            // Device ids: zero-padded counters (outlets add two digits).
            let core: String = s.chars().take(40).collect();
            if core.len() == 40 && core.chars().all(|c| c.is_ascii_hexdigit()) {
                assert!(
                    core.starts_with("000000000000000000000000000000"),
                    "{name}: `{s}` looks like a real device id"
                );
            }
            // MACs: zero-padded counters.
            if s.len() == 17 && s.matches(':').count() == 5 {
                assert!(s.starts_with("00:00:00:00"), "{name}: MAC `{s}`");
            }
            // Tokens: named dummies.
            if s.contains("token") && !s.ends_with("-dummy") {
                assert!(
                    !s.contains('-') || s.starts_with("nbu."),
                    "{name}: token-like `{s}`"
                );
            }
        }
        // Passthrough responses must not carry device coordinates.
        if let Some(data) = v.pointer("/result/responseData").and_then(Value::as_str) {
            let inner: Value = serde_json::from_str(data).unwrap();
            for key in ["longitude_i", "latitude_i"] {
                if let Some(x) = inner.pointer(&format!("/system/get_sysinfo/{key}")) {
                    assert_eq!(x, 0, "{name}: {key} must be scrubbed to 0");
                }
            }
        }
    }
}

fn collect_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => {
            out.push(s.clone());
            // Look inside double-encoded payloads too.
            if let Ok(inner) = serde_json::from_str::<Value>(s) {
                collect_strings(&inner, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        Value::Object(m) => m.values().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}
