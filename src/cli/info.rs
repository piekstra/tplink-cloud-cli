//! `info sysinfo|network|time <device>` — one device's details. (Bare
//! `info` is the family's cli-info/v1 discovery, handled in `crate::run`.)

use clap::Subcommand;
use pk_cli_core::CliError;
use serde_json::{json, Value};

use pk_cli_core::output::emit_one;

use super::emit::{no_data, Ctx};
use crate::models::net_info::DeviceNetInfo;
use crate::models::time::DeviceTime;
use crate::resolve;

#[derive(Subcommand, Debug)]
pub enum InfoCommand {
    /// The device's `get_sysinfo` block (device-sysinfo/v1).
    Sysinfo {
        /// Device name or ID
        device: String,
    },
    /// Wi-Fi network: SSID and signal (device-network/v1).
    Network {
        /// Device name or ID
        device: String,
    },
    /// The device's clock (device-time/v1).
    Time {
        /// Device name or ID
        device: String,
    },
}

fn merge(device: &str, typed: impl serde::Serialize) -> Value {
    let mut out = json!({"device": device});
    if let (Some(obj), Ok(Value::Object(t))) = (out.as_object_mut(), serde_json::to_value(typed)) {
        obj.extend(t);
    }
    out
}

/// `YYYY-MM-DDTHH:MM:SS` when every part is present (the device reports
/// local time with no zone).
pub fn iso_local(t: &DeviceTime) -> Option<String> {
    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        t.year?, t.month?, t.mday?, t.hour?, t.min?, t.sec?
    ))
}

pub async fn handle(ctx: &Ctx<'_>, cmd: &InfoCommand) -> Result<(), CliError> {
    match cmd {
        InfoCommand::Sysinfo { device } => {
            let dev = resolve::resolve_device(ctx, device).await?;
            let info = dev
                .get_sys_info()
                .await?
                .ok_or_else(|| no_data("system info"))?;
            emit_one(
                ctx.json,
                "device-sysinfo",
                json!({"device": dev.alias(), "sys_info": info}),
            );
            Ok(())
        }
        InfoCommand::Network { device } => {
            let dev = resolve::resolve_device(ctx, device).await?;
            let info = dev
                .get_net_info()
                .await?
                .ok_or_else(|| no_data("network info"))?;
            emit_one(
                ctx.json,
                "device-network",
                merge(dev.alias(), DeviceNetInfo::from_json(&info)),
            );
            Ok(())
        }
        InfoCommand::Time { device } => {
            let dev = resolve::resolve_device(ctx, device).await?;
            let raw = dev.get_time().await?.ok_or_else(|| no_data("time"))?;
            let time = DeviceTime::from_json(&raw);
            let mut dto = merge(dev.alias(), &time);
            if let Some(iso) = iso_local(&time) {
                dto["time"] = json!(iso);
            }
            emit_one(ctx.json, "device-time", dto);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_time_renders_iso_when_complete() {
        let t = DeviceTime::from_json(&json!({"year": 2021, "month": 3, "mday": 22,
                                              "hour": 12, "min": 55, "sec": 41}));
        assert_eq!(iso_local(&t).as_deref(), Some("2021-03-22T12:55:41"));
        let partial = DeviceTime::from_json(&json!({"year": 2021}));
        assert!(iso_local(&partial).is_none());
    }
}
