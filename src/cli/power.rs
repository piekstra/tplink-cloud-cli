use clap::Subcommand;
use serde_json::json;

use crate::cli::output::print_json;
use crate::config::RuntimeConfig;
use crate::error::AppError;

use super::super::resolve;

#[derive(Subcommand)]
pub enum PowerCommand {
    /// Turn device on
    On {
        /// Device name or ID
        device: String,
    },

    /// Turn device off
    Off {
        /// Device name or ID
        device: String,
    },

    /// Toggle device power state
    Toggle {
        /// Device name or ID
        device: String,
    },

    /// Check device power status
    Status {
        /// Device name or ID
        device: String,
    },
}

pub async fn handle(cmd: &PowerCommand, config: &RuntimeConfig) -> Result<(), AppError> {
    match cmd {
        PowerCommand::On { device } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            dev.power_on().await?;
            print_json(&json!({"device": dev.alias(), "power": "on"}));
            Ok(())
        }
        PowerCommand::Off { device } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            dev.power_off().await?;
            print_json(&json!({"device": dev.alias(), "power": "off"}));
            Ok(())
        }
        PowerCommand::Toggle { device } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let was_on = dev.is_on().await?;
            dev.toggle().await?;
            let new_state = if was_on == Some(true) { "off" } else { "on" };
            print_json(&json!({"device": dev.alias(), "power": new_state}));
            Ok(())
        }
        PowerCommand::Status { device } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let is_on = dev.is_on().await?;
            let state = match is_on {
                Some(true) => "on",
                Some(false) => "off",
                None => "unknown",
            };
            print_json(&json!({"device": dev.alias(), "power": state}));
            Ok(())
        }
    }
}
