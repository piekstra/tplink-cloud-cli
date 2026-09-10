//! `devices list|get|search` (device-list/v1, device/v1).

use clap::Subcommand;
use pk_cli_core::output::{emit_list, emit_one};
use pk_cli_core::CliError;
use serde_json::json;

use super::emit::Ctx;
use crate::resolve;

/// Columns of the `devices list` table (the DTO carries the rest).
pub const COLUMNS: &[&str] = &[
    "alias",
    "model",
    "category",
    "cloud",
    "status",
    "energy_monitoring",
    "device_id",
];

#[derive(Subcommand, Debug)]
pub enum DevicesCommand {
    /// List every device on the account (outlets of a strip listed as devices).
    #[command(visible_alias = "ls")]
    List,
    /// One device's details, with its live system info.
    Get {
        /// Device name or ID
        device: String,
    },
    /// Devices whose name contains the query (case-insensitive).
    Search {
        /// Search query (partial match on alias)
        query: String,
    },
}

pub async fn handle(ctx: &Ctx<'_>, cmd: &DevicesCommand) -> Result<(), CliError> {
    match cmd {
        DevicesCommand::List => {
            let (devices, _) = resolve::fetch_all_devices(ctx).await?;
            let items = devices.iter().map(|d| d.row()).collect();
            emit_list(ctx.json, "device", items, COLUMNS);
            Ok(())
        }
        DevicesCommand::Search { query } => {
            let (devices, _) = resolve::fetch_all_devices(ctx).await?;
            let q = query.to_lowercase();
            let items = devices
                .iter()
                .filter(|d| d.name().to_lowercase().contains(&q))
                .map(|d| d.row())
                .collect();
            emit_list(ctx.json, "device", items, COLUMNS);
            Ok(())
        }
        DevicesCommand::Get { device } => {
            let dev = resolve::resolve_device(ctx, device).await?;
            let sys_info = dev.get_sys_info().await?;
            let mut dto = json!({
                "alias": dev.alias(),
                "model": dev.info.model(),
                "device_type": format!("{:?}", dev.device_type),
                "category": dev.device_type.category(),
                "cloud": dev.info.cloud_type.map(|c| c.display_name()).unwrap_or("kasa"),
                "device_id": &dev.device_id,
                "is_child": dev.child_id.is_some(),
            });
            if let Some(info) = sys_info {
                dto["sys_info"] = info;
            }
            emit_one(ctx.json, "device", dto);
            Ok(())
        }
    }
}
