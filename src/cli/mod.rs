pub mod auth;
pub mod devices;
pub mod energy;
pub mod info;
pub mod light;
pub mod output;
pub mod power;
pub mod schedule;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "tplc",
    version,
    about = "TP-Link Cloud CLI - control Kasa and Tapo smart home devices"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Output as human-readable table instead of JSON
    #[arg(short = 't', long = "table", global = true)]
    pub table: bool,

    /// Verbose output (show HTTP requests/responses)
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Authenticate with TP-Link Cloud
    Login,

    /// Clear stored authentication tokens
    Logout,

    /// Show authentication status
    Status,

    /// Manage devices
    #[command(subcommand)]
    Devices(devices::DevicesCommand),

    /// Control device power
    #[command(subcommand)]
    Power(power::PowerCommand),

    /// Energy monitoring
    #[command(subcommand)]
    Energy(energy::EnergyCommand),

    /// Light strip controls
    #[command(subcommand)]
    Light(light::LightCommand),

    /// Device schedules
    #[command(subcommand)]
    Schedule(schedule::ScheduleCommand),

    /// Device information
    #[command(subcommand)]
    Info(info::InfoCommand),

    /// Control indicator LED
    Led {
        /// LED state
        #[arg(value_enum)]
        state: LedState,
        /// Device name or ID
        device: String,
    },
}

#[derive(Clone, ValueEnum)]
pub enum LedState {
    On,
    Off,
}

#[derive(Clone, ValueEnum)]
pub enum PowerAction {
    On,
    Off,
}
