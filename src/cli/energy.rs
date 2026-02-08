use chrono::Datelike;
use clap::Subcommand;
use serde_json::json;

use crate::cli::output::print_json;
use crate::config::RuntimeConfig;
use crate::error::AppError;
use crate::models::energy::{CurrentPower, DayPowerSummary, MonthPowerSummary};

use super::super::resolve;

#[derive(Subcommand)]
pub enum EnergyCommand {
    /// Current power usage (realtime)
    Realtime {
        /// Device name or ID
        device: String,
    },

    /// Daily power usage statistics
    Daily {
        /// Device name or ID
        device: String,
        #[arg(long)]
        year: Option<i32>,
        #[arg(long)]
        month: Option<u32>,
    },

    /// Monthly power usage statistics
    Monthly {
        /// Device name or ID
        device: String,
        #[arg(long)]
        year: Option<i32>,
    },

    /// Summary of all energy-monitoring devices
    Summary,
}

pub async fn handle(cmd: &EnergyCommand, config: &RuntimeConfig) -> Result<(), AppError> {
    match cmd {
        EnergyCommand::Realtime { device } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let data = dev.get_power_usage_realtime().await?;
            if let Some(data) = data {
                let power = CurrentPower::from_json(&data);
                print_json(&json!({
                    "device": dev.alias(),
                    "voltage_mv": power.voltage_mv,
                    "current_ma": power.current_ma,
                    "power_mw": power.power_mw,
                    "total_wh": power.total_wh,
                }));
            } else {
                print_json(&json!({"device": dev.alias(), "error": "no data"}));
            }
            Ok(())
        }
        EnergyCommand::Daily {
            device,
            year,
            month,
        } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let now = chrono::Local::now();
            let y = year.unwrap_or(now.year());
            let m = month.unwrap_or(now.month());
            let data = dev.get_power_usage_day(y, m).await?;
            if let Some(data) = data {
                let day_list = data
                    .get("day_list")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let summaries: Vec<serde_json::Value> = day_list
                    .iter()
                    .map(|d| {
                        let s = DayPowerSummary::from_json(d);
                        json!(s)
                    })
                    .collect();
                print_json(&json!({
                    "device": dev.alias(),
                    "year": y,
                    "month": m,
                    "days": summaries,
                }));
            } else {
                print_json(&json!({"device": dev.alias(), "error": "no data"}));
            }
            Ok(())
        }
        EnergyCommand::Monthly { device, year } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let now = chrono::Local::now();
            let y = year.unwrap_or(now.year());
            let data = dev.get_power_usage_month(y).await?;
            if let Some(data) = data {
                let month_list = data
                    .get("month_list")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let summaries: Vec<serde_json::Value> = month_list
                    .iter()
                    .map(|m| {
                        let s = MonthPowerSummary::from_json(m);
                        json!(s)
                    })
                    .collect();
                print_json(&json!({
                    "device": dev.alias(),
                    "year": y,
                    "months": summaries,
                }));
            } else {
                print_json(&json!({"device": dev.alias(), "error": "no data"}));
            }
            Ok(())
        }
        EnergyCommand::Summary => {
            let (devices, _) = resolve::fetch_all_devices(config.verbose).await?;
            let emeter_devices: Vec<_> = devices
                .iter()
                .filter(|(_, dtype, _)| dtype.has_emeter())
                .collect();

            if emeter_devices.is_empty() {
                print_json(
                    &json!({"devices": [], "message": "No energy monitoring devices found"}),
                );
                return Ok(());
            }

            // For summary, we'd need to create Device instances and query each.
            // For now, just list the emeter-capable devices.
            let summaries: Vec<serde_json::Value> = emeter_devices
                .iter()
                .map(|(info, _dtype, child_alias)| {
                    let name = child_alias.as_deref().unwrap_or(info.alias_or_name());
                    json!({
                        "alias": name,
                        "model": info.model(),
                        "device_id": info.id(),
                    })
                })
                .collect();
            print_json(&json!({"emeter_devices": summaries}));
            Ok(())
        }
    }
}
