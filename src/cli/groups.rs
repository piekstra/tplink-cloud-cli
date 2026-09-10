//! Kasa device groups ("rooms" in the Kasa app), which live on TP-Link's
//! IoT cloud (`api.tplinkra.com`), not on the account cloud that serves
//! `getDeviceList`. The envelope is the app SDK's `IOTRequest`; the Kasa
//! account token authenticates it (query `token` + `accountToken`).

use clap::Subcommand;
use pk_cli_core::CliError;
use serde_json::{json, Map, Value};

use pk_cli_core::output::{emit_list, emit_one};

use super::emit::Ctx;
use crate::error::AppError;
use crate::resolve;
use crate::session::TokenSet;

const IOT_HOST: &str = "https://api.tplinkra.com";
/// The Kasa app's client id for the IoT cloud (public, shipped in the APK).
const CLIENT_ID: &str = "46a4d58b-6279-432c-ae23-e115c2db8354";

#[derive(Subcommand, Debug)]
pub enum GroupsCommand {
    /// Device groups (the Kasa app's rooms) with their members (group-list/v1).
    #[command(visible_alias = "ls")]
    List {
        /// Print the IoT cloud's raw response instead (api-response/v1)
        #[arg(long)]
        raw: bool,
    },
    /// Every grouped device with its group, as `device-rooms/v1` for
    /// `ghome audit --expect -` (pass --json when piping).
    Devices,
}

async fn iot_call(
    method: &str,
    data: Value,
    tokens: &TokenSet,
    verbose: bool,
) -> Result<Value, AppError> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let url = format!(
        "{IOT_HOST}/v1/device-groups/{method}?token={}&terminalId={}&clientId={CLIENT_ID}&requestId={request_id}",
        tokens.token, tokens.term_id
    );
    let mut data = data;
    data["uri"] = json!(format!(
        "com.tplinkra.devicegroups.impl.{}",
        request_class(method)
    ));
    let body = json!({
        "requestId": request_id,
        "module": "device-groups",
        "method": method,
        "iotContext": {
            "userContext": {
                "email": tokens.username,
                "accountToken": tokens.token,
                "terminalId": tokens.term_id,
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
        message: format!(
            "HTTP {}: {}",
            status.as_u16(),
            text.chars().take(300).collect::<String>()
        ),
        error_code: Some(status.as_u16() as i32),
    })?;
    if v.get("status").and_then(Value::as_str) == Some("SUCCESS")
        || v.get("data").is_some() && v.get("errorCode").is_none()
    {
        Ok(v)
    } else {
        Err(AppError::Api {
            message: format!(
                "{method}: {}",
                v.get("msg")
                    .or(v.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or(&text.chars().take(300).collect::<String>())
            ),
            error_code: v.get("errorCode").and_then(Value::as_i64).map(|n| n as i32),
        })
    }
}

/// One `device-rooms/v1` item. `name` is a required string in that
/// contract, so an unknown name is omitted rather than emitted as null;
/// `room` is omitted (never null) when the vendor app files the device in
/// no room, so a consumer can report the gap.
pub fn device_room_row(id: &str, name: Option<String>, room: Option<&str>) -> Value {
    let mut row = Map::new();
    row.insert("id".into(), json!(id));
    if let Some(n) = name {
        row.insert("name".into(), json!(n));
    }
    if let Some(r) = room {
        row.insert("room".into(), json!(r));
    }
    row.insert("source".into(), json!("tplink"));
    Value::Object(row)
}

fn request_class(method: &str) -> String {
    let mut c = method.chars();
    match c.next() {
        Some(f) => format!("{}{}Request", f.to_uppercase(), c.as_str()),
        None => "Request".into(),
    }
}

pub async fn handle(ctx: &Ctx<'_>, cmd: &GroupsCommand) -> Result<(), CliError> {
    let tokens = ctx.session()?;
    let v = iot_call(
        "listDeviceGroups",
        json!({"paginator": {"from": 0, "pageSize": 100}}),
        &tokens,
        ctx.verbose,
    )
    .await?;
    let groups: Vec<Value> = v
        .pointer("/data/listing")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    match cmd {
        GroupsCommand::List { raw: true } => {
            emit_one(ctx.json, "api-response", v);
            Ok(())
        }
        GroupsCommand::List { raw: false } => {
            let items: Vec<Value> = groups
                .iter()
                .map(|g| {
                    let ids: Vec<Value> = g
                        .get("items")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(|i| i.get("id").cloned()).collect())
                        .unwrap_or_default();
                    json!({
                        "id": g.get("id"),
                        "name": g.get("alias"),
                        "type": g.get("type"),
                        "devices": ids.len(),
                        "device_ids": ids,
                    })
                })
                .collect();
            emit_list(ctx.json, "group", items, &["id", "name", "type", "devices"]);
            Ok(())
        }
        GroupsCommand::Devices => {
            let (devices, _) = resolve::fetch_all_devices(ctx).await?;
            let name_of = |id: &str| {
                devices
                    .iter()
                    .find(|d| d.child_id.is_none() && d.info.id() == id)
                    .map(|d| d.name().to_string())
            };
            let mut items = Vec::new();
            for g in &groups {
                let room = g.get("alias").and_then(Value::as_str).unwrap_or("");
                for it in g
                    .get("items")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(id) = it.get("id").and_then(Value::as_str) {
                        items.push(device_room_row(id, name_of(id), Some(room)));
                    }
                }
            }
            // `device-rooms/v1` (smart-home/v1 profile): what `ghome audit
            // --expect -` consumes.
            emit_list(ctx.json, "device-rooms", items, &["name", "room", "id"]);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_room_rows_omit_an_unknown_name() {
        let named = device_room_row("d1", Some("Lamp".into()), Some("Office"));
        assert_eq!(named["name"], "Lamp");
        assert_eq!(named["room"], "Office");
        let anon = device_room_row("d2", None, Some("Office"));
        assert!(anon.get("name").is_none(), "name must be absent, not null");
        let unfiled = device_room_row("d3", Some("Bulb".into()), None);
        assert!(
            unfiled.get("room").is_none(),
            "room must be absent, not null"
        );
        assert_eq!(anon["source"], "tplink");
    }

    #[test]
    fn request_class_capitalises_the_method() {
        assert_eq!(request_class("listDeviceGroups"), "ListDeviceGroupsRequest");
        assert_eq!(request_class(""), "Request");
    }
}
