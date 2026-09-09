//! Kasa device groups ("rooms" in the Kasa app), which live on TP-Link's
//! IoT cloud (`api.tplinkra.com`), not on the account cloud that serves
//! `getDeviceList`. The envelope is the app SDK's `IOTRequest`; the Kasa
//! account token authenticates it (query `token` + `accountToken`).

use clap::Subcommand;
use serde_json::{json, Value};

use crate::auth::credentials;
use crate::cli::output::print_json;
use crate::config::RuntimeConfig;
use crate::error::AppError;
use crate::resolve;

const IOT_HOST: &str = "https://api.tplinkra.com";
/// The Kasa app's client id for the IoT cloud (public, shipped in the APK).
const CLIENT_ID: &str = "46a4d58b-6279-432c-ae23-e115c2db8354";

#[derive(Subcommand)]
pub enum GroupsCommand {
    /// List device groups (the Kasa app's rooms) with their members
    List {
        /// Print the IoT cloud's raw response instead
        #[arg(long)]
        raw: bool,
    },
    /// Every grouped device with its group, as `device-rooms/v1` for
    /// `ghome audit --expect -`
    Devices,
}

async fn iot_call(
    method: &str,
    data: Value,
    auth: &credentials::AuthContext,
    verbose: bool,
) -> Result<Value, AppError> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let url = format!(
        "{IOT_HOST}/v1/device-groups/{method}?token={}&terminalId={}&clientId={CLIENT_ID}&requestId={request_id}",
        auth.token, auth.term_id
    );
    let mut data = data;
    data["uri"] = json!(format!("com.tplinkra.devicegroups.impl.{}", request_class(method)));
    let body = json!({
        "requestId": request_id,
        "module": "device-groups",
        "method": method,
        "iotContext": {
            "userContext": {
                "email": auth.username,
                "accountToken": auth.token,
                "terminalId": auth.term_id,
                "app": { "appType": "Kasa_Android", "appClientId": CLIENT_ID }
            }
        },
        "data": data,
    });
    if verbose {
        eprintln!("POST {IOT_HOST}/v1/device-groups/{method}");
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    if verbose {
        eprintln!("HTTP {} ({} bytes)", status.as_u16(), text.len());
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| AppError::Api {
        message: format!("HTTP {}: {}", status.as_u16(), text.chars().take(300).collect::<String>()),
        error_code: Some(status.as_u16() as i32),
    })?;
    if v.get("status").and_then(Value::as_str) == Some("SUCCESS") || v.get("data").is_some() && v.get("errorCode").is_none() {
        Ok(v)
    } else {
        Err(AppError::Api {
            message: format!(
                "{method}: {}",
                v.get("msg").or(v.get("message")).and_then(Value::as_str).unwrap_or(&text.chars().take(300).collect::<String>())
            ),
            error_code: v.get("errorCode").and_then(Value::as_i64).map(|n| n as i32),
        })
    }
}

fn request_class(method: &str) -> String {
    let mut c = method.chars();
    match c.next() {
        Some(f) => format!("{}{}Request", f.to_uppercase(), c.as_str()),
        None => "Request".into(),
    }
}

pub async fn handle(cmd: &GroupsCommand, config: &RuntimeConfig) -> Result<(), AppError> {
    let auth = credentials::get_auth_context(config.verbose).await?;
    let v = iot_call(
        "listDeviceGroups",
        json!({"paginator": {"from": 0, "pageSize": 100}}),
        &auth,
        config.verbose,
    )
    .await?;
    let groups: Vec<Value> = v
        .pointer("/data/listing")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    match cmd {
        GroupsCommand::List { raw: true } => {
            print_json(&v);
            Ok(())
        }
        GroupsCommand::List { raw: false } => {
            let items: Vec<Value> = groups
                .iter()
                .map(|g| {
                    json!({
                        "id": g.get("id"),
                        "name": g.get("alias"),
                        "type": g.get("type"),
                        "devices": g.get("items").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0),
                        "device_ids": g.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(|i| i.get("id").cloned()).collect::<Vec<_>>()).unwrap_or_default(),
                    })
                })
                .collect();
            print_json(&json!({"groups": items}));
            Ok(())
        }
        GroupsCommand::Devices => {
            let (devices, _) = resolve::fetch_all_devices(config.verbose).await?;
            let name_of = |id: &str| {
                devices
                    .iter()
                    .find(|(info, _, _)| info.id() == id)
                    .map(|(info, _, child)| child.clone().unwrap_or_else(|| info.alias_or_name().to_string()))
            };
            let mut items = Vec::new();
            for g in &groups {
                let room = g.get("alias").and_then(Value::as_str).unwrap_or("");
                for it in g.get("items").and_then(Value::as_array).into_iter().flatten() {
                    if let Some(id) = it.get("id").and_then(Value::as_str) {
                        items.push(json!({
                            "id": id,
                            "name": name_of(id),
                            "room": room,
                            "source": "tplink",
                        }));
                    }
                }
            }
            print_json(&json!({"schema": "device-rooms/v1", "items": items}));
            Ok(())
        }
    }
}
