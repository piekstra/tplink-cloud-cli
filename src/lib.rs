pub mod api;
pub mod auth;
pub mod cli;
pub mod config;
pub mod error;
pub mod models;
pub mod resolve;

use cli::output::print_error;
use config::{OutputMode, RuntimeConfig};
use error::AppError;

pub async fn run(cli_args: cli::Cli) -> i32 {
    let config = RuntimeConfig {
        output_mode: if cli_args.table {
            OutputMode::Table
        } else {
            OutputMode::Json
        },
        verbose: cli_args.verbose,
    };

    let result = dispatch(cli_args.command, &config).await;

    match result {
        Ok(()) => 0,
        Err(err) => {
            print_error(&err);
            err.exit_code()
        }
    }
}

async fn dispatch(command: cli::Commands, config: &RuntimeConfig) -> Result<(), AppError> {
    match command {
        cli::Commands::Login => cli::auth::handle_login(config).await,
        cli::Commands::Logout => cli::auth::handle_logout(config).await,
        cli::Commands::Status => cli::auth::handle_status(config).await,
        cli::Commands::Devices(cmd) => cli::devices::handle(&cmd, config).await,
        cli::Commands::Power(cmd) => cli::power::handle(&cmd, config).await,
        cli::Commands::Energy(cmd) => cli::energy::handle(&cmd, config).await,
        cli::Commands::Light(cmd) => cli::light::handle(&cmd, config).await,
        cli::Commands::Schedule(cmd) => cli::schedule::handle(&cmd, config).await,
        cli::Commands::Info(cmd) => cli::info::handle(&cmd, config).await,
        cli::Commands::Led { state, device } => {
            let dev = resolve::resolve_device(&device, config.verbose).await?;
            let on = matches!(state, cli::LedState::On);
            dev.set_led_state(on).await?;
            let state_str = if on { "on" } else { "off" };
            cli::output::print_json(&serde_json::json!({"device": dev.alias(), "led": state_str}));
            Ok(())
        }
    }
}
