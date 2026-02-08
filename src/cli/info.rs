use clap::Subcommand;
use serde_json::json;

use crate::cli::output::print_json;
use crate::config::RuntimeConfig;
use crate::error::AppError;

use super::super::resolve;

#[derive(Subcommand)]
pub enum InfoCommand {
    /// System information
    Sysinfo {
        /// Device name or ID
        device: String,
    },

    /// WiFi network information
    Network {
        /// Device name or ID
        device: String,
    },

    /// Device time
    Time {
        /// Device name or ID
        device: String,
    },
}

pub async fn handle(cmd: &InfoCommand, config: &RuntimeConfig) -> Result<(), AppError> {
    match cmd {
        InfoCommand::Sysinfo { device } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let info = dev.get_sys_info().await?;
            if let Some(info) = info {
                print_json(&json!({"device": dev.alias(), "sys_info": info}));
            } else {
                print_json(&json!({"device": dev.alias(), "error": "no data"}));
            }
            Ok(())
        }
        InfoCommand::Network { device } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let info = dev.get_net_info().await?;
            if let Some(info) = info {
                print_json(&json!({"device": dev.alias(), "net_info": info}));
            } else {
                print_json(&json!({"device": dev.alias(), "error": "no data"}));
            }
            Ok(())
        }
        InfoCommand::Time { device } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let time = dev.get_time().await?;
            if let Some(time) = time {
                print_json(&json!({"device": dev.alias(), "time": time}));
            } else {
                print_json(&json!({"device": dev.alias(), "error": "no data"}));
            }
            Ok(())
        }
    }
}
