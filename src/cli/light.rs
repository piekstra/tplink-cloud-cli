use clap::Subcommand;
use serde_json::json;

use crate::cli::output::print_json;
use crate::config::RuntimeConfig;
use crate::error::AppError;

use super::super::resolve;

#[derive(Subcommand)]
pub enum LightCommand {
    /// Set brightness (0-100)
    Brightness {
        /// Device name or ID
        device: String,
        /// Brightness level
        #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
        level: u8,
    },

    /// Set color by HSB
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

    /// Set color temperature (2500-9000K)
    Temp {
        /// Device name or ID
        device: String,
        /// Color temperature in Kelvin
        #[arg(value_parser = clap::value_parser!(u16).range(2500..=9000))]
        kelvin: u16,
        /// Brightness (0-100)
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        brightness: Option<u8>,
    },

    /// Get current light state
    State {
        /// Device name or ID
        device: String,
    },
}

pub async fn handle(cmd: &LightCommand, config: &RuntimeConfig) -> Result<(), AppError> {
    match cmd {
        LightCommand::Brightness { device, level } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            dev.set_brightness(*level).await?;
            print_json(&json!({"device": dev.alias(), "brightness": level}));
            Ok(())
        }
        LightCommand::Color {
            device,
            hue,
            saturation,
            brightness,
        } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            dev.set_color(*hue, *saturation, *brightness).await?;
            print_json(&json!({
                "device": dev.alias(),
                "hue": hue,
                "saturation": saturation,
                "brightness": brightness,
            }));
            Ok(())
        }
        LightCommand::Temp {
            device,
            kelvin,
            brightness,
        } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            dev.set_color_temp(*kelvin, *brightness).await?;
            print_json(&json!({
                "device": dev.alias(),
                "color_temp": kelvin,
                "brightness": brightness,
            }));
            Ok(())
        }
        LightCommand::State { device } => {
            let dev = resolve::resolve_device(device, config.verbose).await?;
            let state = dev.get_light_state().await?;
            if let Some(state) = state {
                print_json(&json!({"device": dev.alias(), "light_state": state}));
            } else {
                print_json(&json!({"device": dev.alias(), "error": "no data"}));
            }
            Ok(())
        }
    }
}
