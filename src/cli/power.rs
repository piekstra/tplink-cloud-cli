//! `power on|off|toggle|status` (power-state/v1).

use clap::Subcommand;
use pk_cli_core::output::emit_one;
use pk_cli_core::CliError;
use serde_json::json;

use super::emit::Ctx;
use super::smart;
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
    let (devices, tokens) = resolve::fetch_all_devices(ctx).await?;
    let found = resolve::pick(&devices, name)?;
    if found.info.uses_shadow() {
        return handle_shadow(ctx, cmd, requested, found).await;
    }
    let dev = resolve::build_device(found, &tokens, ctx.verbose)?;
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

/// A newer Kasa device (`SMART.KASA*`): read and switched through its
/// thing shadow, and a change is read back before it is reported.
async fn handle_shadow(
    ctx: &Ctx<'_>,
    cmd: &PowerCommand,
    requested: Option<bool>,
    found: &resolve::Listed,
) -> Result<(), CliError> {
    let target = match (cmd, requested) {
        (_, Some(on)) => Some(on),
        (PowerCommand::Toggle { .. }, None) => match smart::power_status(ctx, found).await? {
            Some(was_on) => Some(!was_on),
            None => {
                return Err(CliError::Upstream(
                    "Could not determine device power state".into(),
                ))
            }
        },
        _ => None,
    };
    let state = match target {
        Some(on) => smart::set_power(ctx, found, on).await?,
        None => smart::power_status(ctx, found).await?,
    };
    let power = match state {
        Some(true) => "on",
        Some(false) => "off",
        None => "unknown",
    };
    emit_one(
        ctx.json,
        "power-state",
        json!({"device": found.name(), "device_id": found.info.id(), "power": power}),
    );
    Ok(())
}
