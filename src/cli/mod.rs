pub mod api;
pub mod auth;
pub mod devices;
pub mod energy;
pub mod groups;
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
    Login {
        /// Read the password from stdin (the scriptable path:
        /// `op read … | tplc login --stdin --username you@example.com`)
        #[arg(long)]
        stdin: bool,
        /// Account email (with --stdin; otherwise prompted)
        #[arg(long)]
        username: Option<String>,
        /// The MFA code TP-Link emailed, to resume a login that stopped for one
        #[arg(long)]
        mfa_code: Option<String>,
    },

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
    /// Kasa device groups — the app's rooms (IoT cloud)
    #[command(subcommand)]
    Groups(groups::GroupsCommand),
    /// Call any cloud method by name and print the raw response (for methods
    /// the CLI doesn't model yet). Example: `tplc api listDeviceGroups`.
    Api {
        /// Method name, e.g. getDeviceList, listDeviceGroups
        method: String,
        /// JSON object for the method's params
        #[arg(long)]
        params: Option<String>,
        /// Which cloud to call: kasa or tapo
        #[arg(long, default_value = "kasa")]
        cloud: String,
    },
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
