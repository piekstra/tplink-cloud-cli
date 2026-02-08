use clap::Subcommand;
use serde_json::json;
use tabled::Tabled;

use crate::cli::output::{print_json, print_table};
use crate::config::{OutputMode, RuntimeConfig};
use crate::error::AppError;

use super::super::resolve;

#[derive(Subcommand)]
pub enum DevicesCommand {
    /// List all devices
    List,

    /// Get device details
    Get {
        /// Device name or ID
        device: String,
    },

    /// Search devices by partial name
    Search {
        /// Search query (partial match on alias)
        query: String,
    },
}

#[derive(Tabled)]
struct DeviceRow {
    #[tabled(rename = "NAME")]
    name: String,
    #[tabled(rename = "MODEL")]
    model: String,
    #[tabled(rename = "TYPE")]
    category: String,
    #[tabled(rename = "STATUS")]
    status: String,
    #[tabled(rename = "EMETER")]
    emeter: String,
    #[tabled(rename = "DEVICE ID")]
    device_id: String,
}

pub async fn handle(cmd: &DevicesCommand, config: &RuntimeConfig) -> Result<(), AppError> {
    match cmd {
        DevicesCommand::List => handle_list(config).await,
        DevicesCommand::Get { device } => handle_get(device, config).await,
        DevicesCommand::Search { query } => handle_search(query, config).await,
    }
}

async fn handle_list(config: &RuntimeConfig) -> Result<(), AppError> {
    let (devices, _auth) = resolve::fetch_all_devices(config.verbose).await?;

    if config.output_mode == OutputMode::Table {
        let rows: Vec<DeviceRow> = devices
            .iter()
            .map(|(info, dtype, child_alias)| {
                let name = child_alias
                    .as_deref()
                    .unwrap_or(info.alias_or_name())
                    .to_string();
                DeviceRow {
                    name,
                    model: info.model().to_string(),
                    category: dtype.category().to_string(),
                    status: if info.status == Some(1) {
                        "online"
                    } else {
                        "offline"
                    }
                    .to_string(),
                    emeter: if dtype.has_emeter() { "yes" } else { "no" }.to_string(),
                    device_id: info.id().to_string(),
                }
            })
            .collect();
        print_table(&rows);
    } else {
        let json_devices: Vec<serde_json::Value> = devices
            .iter()
            .map(|(info, dtype, child_alias)| {
                let name = child_alias.as_deref().unwrap_or(info.alias_or_name());
                json!({
                    "alias": name,
                    "model": info.model(),
                    "device_type": format!("{:?}", dtype),
                    "category": dtype.category(),
                    "device_id": info.id(),
                    "status": if info.status == Some(1) { "online" } else { "offline" },
                    "energy_monitoring": dtype.has_emeter(),
                })
            })
            .collect();
        print_json(&json!(json_devices));
    }

    Ok(())
}

async fn handle_get(device_name: &str, config: &RuntimeConfig) -> Result<(), AppError> {
    let device = resolve::resolve_device(device_name, config.verbose).await?;

    let sys_info = device.get_sys_info().await?;

    let mut result = json!({
        "alias": device.alias(),
        "model": device.info.model(),
        "device_type": format!("{:?}", device.device_type),
        "category": device.device_type.category(),
        "device_id": &device.device_id,
        "is_child": device.child_id.is_some(),
    });

    if let Some(info) = sys_info {
        result["sys_info"] = info;
    }

    print_json(&result);

    Ok(())
}

async fn handle_search(query: &str, config: &RuntimeConfig) -> Result<(), AppError> {
    let (devices, _auth) = resolve::fetch_all_devices(config.verbose).await?;

    let query_lower = query.to_lowercase();
    let matching: Vec<_> = devices
        .iter()
        .filter(|(info, _, child_alias)| {
            let name = child_alias.as_deref().unwrap_or(info.alias_or_name());
            name.to_lowercase().contains(&query_lower)
        })
        .collect();

    if config.output_mode == OutputMode::Table {
        let rows: Vec<DeviceRow> = matching
            .iter()
            .map(|(info, dtype, child_alias)| {
                let name = child_alias
                    .as_deref()
                    .unwrap_or(info.alias_or_name())
                    .to_string();
                DeviceRow {
                    name,
                    model: info.model().to_string(),
                    category: dtype.category().to_string(),
                    status: if info.status == Some(1) {
                        "online"
                    } else {
                        "offline"
                    }
                    .to_string(),
                    emeter: if dtype.has_emeter() { "yes" } else { "no" }.to_string(),
                    device_id: info.id().to_string(),
                }
            })
            .collect();
        print_table(&rows);
    } else {
        let json_devices: Vec<serde_json::Value> = matching
            .iter()
            .map(|(info, dtype, child_alias)| {
                let name = child_alias.as_deref().unwrap_or(info.alias_or_name());
                json!({
                    "alias": name,
                    "model": info.model(),
                    "device_type": format!("{:?}", dtype),
                    "device_id": info.id(),
                    "status": if info.status == Some(1) { "online" } else { "offline" },
                })
            })
            .collect();
        print_json(&json!(json_devices));
    }

    Ok(())
}
