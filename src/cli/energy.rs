//! `energy realtime|daily|monthly|summary` for devices with a meter.

use chrono::Datelike;
use clap::Subcommand;
use pk_cli_core::CliError;
use serde_json::{json, Map, Value};

use pk_cli_core::output::{emit_list, emit_one};

use super::emit::{emit_headed_list, no_data, Ctx};
use crate::models::energy::{CurrentPower, DayPowerSummary, MonthPowerSummary};
use crate::resolve;

#[derive(Subcommand, Debug)]
pub enum EnergyCommand {
    /// Instantaneous readings (energy-realtime/v1).
    Realtime {
        /// Device name or ID
        device: String,
    },
    /// Per-day totals for a month (energy-day-list/v1); defaults to this month.
    Daily {
        /// Device name or ID
        device: String,
        #[arg(long)]
        year: Option<i32>,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=12))]
        month: Option<u32>,
    },
    /// Per-month totals for a year (energy-month-list/v1); defaults to this year.
    Monthly {
        /// Device name or ID
        device: String,
        #[arg(long)]
        year: Option<i32>,
    },
    /// Every device that has a meter (energy-device-list/v1); no readings.
    Summary,
}

fn rows<T: serde::Serialize>(data: &Value, key: &str, parse: impl Fn(&Value) -> T) -> Vec<Value> {
    data.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|d| serde_json::to_value(parse(d)).unwrap_or(Value::Null))
                .collect()
        })
        .unwrap_or_default()
}

pub async fn handle(ctx: &Ctx<'_>, cmd: &EnergyCommand) -> Result<(), CliError> {
    match cmd {
        EnergyCommand::Realtime { device } => {
            let dev = resolve::resolve_device(ctx, device).await?;
            let data = dev
                .get_power_usage_realtime()
                .await?
                .ok_or_else(|| no_data("realtime reading"))?;
            let power = CurrentPower::from_json(&data);
            let mut dto = json!({"device": dev.alias()});
            if let (Some(obj), Ok(Value::Object(p))) =
                (dto.as_object_mut(), serde_json::to_value(&power))
            {
                obj.extend(p);
            }
            emit_one(ctx.json, "energy-realtime", dto);
            Ok(())
        }
        EnergyCommand::Daily {
            device,
            year,
            month,
        } => {
            let now = chrono::Local::now();
            let y = year.unwrap_or(now.year());
            let m = month.unwrap_or(now.month());
            let dev = resolve::resolve_device(ctx, device).await?;
            let data = dev
                .get_power_usage_day(y, m)
                .await?
                .ok_or_else(|| no_data("daily statistics"))?;
            let items = rows(&data, "day_list", DayPowerSummary::from_json);
            let mut head = Map::new();
            head.insert("device".into(), json!(dev.alias()));
            head.insert("year".into(), json!(y));
            head.insert("month".into(), json!(m));
            emit_headed_list(ctx.json, "energy-day", head, items, &["day", "energy_wh"]);
            Ok(())
        }
        EnergyCommand::Monthly { device, year } => {
            let y = year.unwrap_or(chrono::Local::now().year());
            let dev = resolve::resolve_device(ctx, device).await?;
            let data = dev
                .get_power_usage_month(y)
                .await?
                .ok_or_else(|| no_data("monthly statistics"))?;
            let items = rows(&data, "month_list", MonthPowerSummary::from_json);
            let mut head = Map::new();
            head.insert("device".into(), json!(dev.alias()));
            head.insert("year".into(), json!(y));
            emit_headed_list(
                ctx.json,
                "energy-month",
                head,
                items,
                &["month", "energy_wh"],
            );
            Ok(())
        }
        EnergyCommand::Summary => {
            let (devices, _) = resolve::fetch_all_devices(ctx).await?;
            let items = devices
                .iter()
                .filter(|d| d.dtype.has_emeter())
                .map(|d| {
                    json!({
                        "alias": d.name(),
                        "model": d.info.model(),
                        "cloud": d.cloud().display_name(),
                        "device_id": d.info.id(),
                    })
                })
                .collect();
            emit_list(
                ctx.json,
                "energy-device",
                items,
                &["alias", "model", "cloud", "device_id"],
            );
            Ok(())
        }
    }
}
