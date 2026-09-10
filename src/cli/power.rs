//! `power on|off|toggle|status` (power-state/v1).

use clap::Subcommand;
use pk_cli_core::output::emit_one;
use pk_cli_core::CliError;
use serde_json::json;

use super::emit::Ctx;
use crate::resolve;

#[derive(Subcommand, Debug)]
pub enum PowerCommand {
    /// Turn a device on.
    On {
        /// Device name or ID
        device: String,
    },
    /// Turn a device off.
    Off {
        /// Device name or ID
        device: String,
    },
    /// Toggle a device's power state.
    Toggle {
        /// Device name or ID
        device: String,
    },
    /// Report whether a device is on.
    Status {
        /// Device name or ID
        device: String,
    },
}

pub async fn handle(ctx: &Ctx<'_>, cmd: &PowerCommand) -> Result<(), CliError> {
    let (name, requested) = match cmd {
        PowerCommand::On { device } => (device, Some(true)),
        PowerCommand::Off { device } => (device, Some(false)),
        PowerCommand::Toggle { device } | PowerCommand::Status { device } => (device, None),
    };
    let dev = resolve::resolve_device(ctx, name).await?;
    let power = match (cmd, requested) {
        (_, Some(true)) => {
            dev.power_on().await?;
            "on"
        }
        (_, Some(false)) => {
            dev.power_off().await?;
            "off"
        }
        (PowerCommand::Toggle { .. }, None) => {
            let was_on = dev.is_on().await?;
            dev.toggle().await?;
            if was_on == Some(true) {
                "off"
            } else {
                "on"
            }
        }
        _ => match dev.is_on().await? {
            Some(true) => "on",
            Some(false) => "off",
            None => "unknown",
        },
    };
    emit_one(
        ctx.json,
        "power-state",
        json!({"device": dev.alias(), "device_id": dev.device_id, "power": power}),
    );
    Ok(())
}
