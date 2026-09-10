//! `light brightness|color|temp|state` (light-state/v1).

use clap::Subcommand;
use pk_cli_core::CliError;
use serde_json::{json, Value};

use pk_cli_core::output::emit_one;

use super::emit::{no_data, Ctx};
use crate::models::light_state::LightState;
use crate::resolve;

#[derive(Subcommand, Debug)]
pub enum LightCommand {
    /// Set brightness (0-100); also turns the light on.
    Brightness {
        /// Device name or ID
        device: String,
        /// Brightness level
        #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
        level: u8,
    },
    /// Set colour by hue/saturation (and optional brightness).
    Color {
        /// Device name or ID
        device: String,
        /// Hue (0-360)
        #[arg(long, value_parser = clap::value_parser!(u16).range(0..=360))]
        hue: u16,
        /// Saturation (0-100)
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        saturation: u8,
        /// Brightness (0-100)
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        brightness: Option<u8>,
    },
    /// Set colour temperature (2500-9000K) and optional brightness.
    Temp {
        /// Device name or ID
        device: String,
        /// Colour temperature in Kelvin
        #[arg(value_parser = clap::value_parser!(u16).range(2500..=9000))]
        kelvin: u16,
        /// Brightness (0-100)
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        brightness: Option<u8>,
    },
    /// Read the current light state.
    State {
        /// Device name or ID
        device: String,
    },
}

/// The DTO: `device` plus the typed light state the device reported (or,
/// when a set command got no echo back, the values that were requested).
fn dto(device: &str, reported: Option<Value>, requested: Value) -> Value {
    let state = match reported {
        Some(v) => serde_json::to_value(LightState::from_json(&v)).unwrap_or(requested),
        None => requested,
    };
    let mut out = json!({"device": device});
    if let (Some(obj), Value::Object(s)) = (out.as_object_mut(), state) {
        obj.extend(s);
    }
    out
}

pub async fn handle(ctx: &Ctx<'_>, cmd: &LightCommand) -> Result<(), CliError> {
    let (device, requested) = match cmd {
        LightCommand::Brightness { device, level } => {
            (device, json!({"on_off": 1, "brightness": level}))
        }
        LightCommand::Color {
            device,
            hue,
            saturation,
            brightness,
        } => (
            device,
            json!({"on_off": 1, "hue": hue, "saturation": saturation, "brightness": brightness}),
        ),
        LightCommand::Temp {
            device,
            kelvin,
            brightness,
        } => (
            device,
            json!({"on_off": 1, "color_temp": kelvin, "brightness": brightness}),
        ),
        LightCommand::State { device } => (device, Value::Null),
    };
    let dev = resolve::resolve_device(ctx, device).await?;
    let reported = match cmd {
        LightCommand::Brightness { level, .. } => dev.set_brightness(*level).await?,
        LightCommand::Color {
            hue,
            saturation,
            brightness,
            ..
        } => dev.set_color(*hue, *saturation, *brightness).await?,
        LightCommand::Temp {
            kelvin, brightness, ..
        } => dev.set_color_temp(*kelvin, *brightness).await?,
        LightCommand::State { .. } => Some(
            dev.get_light_state()
                .await?
                .ok_or_else(|| no_data("light state"))?,
        ),
    };
    emit_one(
        ctx.json,
        "light-state",
        dto(dev.alias(), reported, requested),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_state_is_typed_and_nulls_are_omitted() {
        let v = dto(
            "Strip",
            Some(json!({"on_off": 1, "brightness": 40, "mode": "normal", "err_code": 0})),
            Value::Null,
        );
        assert_eq!(v["device"], "Strip");
        assert_eq!(v["brightness"], 40);
        assert!(v.get("err_code").is_none());
        assert!(
            v.get("hue").is_none(),
            "absent fields are omitted, not null"
        );
    }

    #[test]
    fn a_silent_device_echoes_the_request() {
        let v = dto("Bulb", None, json!({"on_off": 1, "color_temp": 2700}));
        assert_eq!(v["color_temp"], 2700);
    }
}
