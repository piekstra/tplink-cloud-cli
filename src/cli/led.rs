//! `led on|off <DEVICE>` — a device's indicator LED (led-state/v1).

use pk_cli_core::output::emit_one;
use pk_cli_core::CliError;
use serde_json::json;

use super::emit::Ctx;
use super::LedState;
use crate::resolve;

pub async fn handle(ctx: &Ctx<'_>, state: LedState, device: &str) -> Result<(), CliError> {
    let dev = resolve::resolve_device(ctx, device).await?;
    let on = matches!(state, LedState::On);
    dev.set_led_state(on).await?;
    emit_one(
        ctx.json,
        "led-state",
        json!({
            "device": dev.alias(),
            "device_id": dev.device_id,
            "led": if on { "on" } else { "off" },
        }),
    );
    Ok(())
}
