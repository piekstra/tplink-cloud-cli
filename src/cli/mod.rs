//! The command tree (clap derive). Handlers live in the sibling modules;
//! `crate::run` dispatches.

pub mod api;
pub mod auth;
pub mod config;
pub mod devices;
pub mod emit;
pub mod energy;
pub mod groups;
pub mod info;
pub mod light;
pub mod power;
pub mod rooms;
pub mod schedule;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use pk_cli_auth::{LogoutArgs, SetCredentialArgs};
use pk_cli_core::CommonArgs;
use pk_cli_selfupdate::SelfUpdateArgs;

/// TP-Link Kasa and Tapo devices from the terminal (conforms to piekstra-cli/1).
#[derive(Parser, Debug)]
#[command(name = "tplc", version, about, long_about = None)]
pub struct Cli {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Override the config file location.
    #[arg(long, global = true, value_name = "PATH", env = "TPLC_CONFIG")]
    pub config: Option<PathBuf>,

    /// Deprecated no-op: text is now the default output (use --json for JSON).
    #[arg(short = 't', long = "table", global = true, hide = true)]
    pub table: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Log in, log out, and report session state.
    #[command(subcommand)]
    Auth(AuthCmd),
    /// Alias of `auth login` (kept for one major version).
    #[command(hide = true)]
    Login(auth::LoginOpts),
    /// Alias of `auth logout` (kept for one major version).
    #[command(hide = true)]
    Logout(LogoutArgs),
    /// Alias of `auth status` (kept for one major version).
    #[command(hide = true)]
    Status,
    /// Non-secret settings.
    #[command(subcommand)]
    Config(ConfigCmd),
    /// Devices on the account: list, get, search.
    #[command(subcommand)]
    Devices(devices::DevicesCommand),
    /// Switch devices on and off.
    #[command(subcommand)]
    Power(power::PowerCommand),
    /// Energy monitoring (devices with a meter).
    #[command(subcommand)]
    Energy(energy::EnergyCommand),
    /// Bulbs and light strips: brightness, colour, temperature.
    #[command(subcommand)]
    Light(light::LightCommand),
    /// Per-device schedule rules.
    #[command(subcommand)]
    Schedule(schedule::ScheduleCommand),
    /// Capability discovery (cli-info/v1), or one device's details.
    ///
    /// Bare `info` prints the machine-readable cli-info/v1 document. With a
    /// subcommand (`sysinfo`, `network`, `time`) it reads one device.
    Info {
        #[command(subcommand)]
        cmd: Option<info::InfoCommand>,
    },
    /// Switch a device's indicator LED on or off.
    Led {
        /// LED state
        #[arg(value_enum)]
        state: LedState,
        /// Device name or ID
        device: String,
    },
    /// Tapo homes and rooms: list, audit shape, move, create, rename, delete.
    #[command(subcommand)]
    Rooms(rooms::RoomsCommand),
    /// Kasa device groups — the Kasa app's rooms (IoT cloud).
    #[command(subcommand)]
    Groups(groups::GroupsCommand),
    /// Call any cloud method by name and print the raw response.
    ///
    /// A cloud-RPC passthrough for methods the CLI doesn't model yet, e.g.
    /// `tplc api listDeviceGroups --params '{"paginator":{"from":0,"pageSize":50}}'`.
    /// Not the family's HTTP `api <VERB> <PATH>` form: the TP-Link cloud has
    /// method names, not paths.
    Api(api::ApiArgs),
    /// Update to the latest release from GitHub.
    SelfUpdate(SelfUpdateArgs),
    /// Print a shell completion script.
    Completions { shell: Shell },
}

#[derive(Subcommand, Debug)]
pub enum AuthCmd {
    /// Log in to the Kasa and Tapo clouds and store the session in the OS keychain.
    Login(auth::LoginOpts),
    /// Report session state (auth-status/v1). Works logged out.
    Status,
    /// Clear the stored session; --forget also clears the saved identity/config.
    Logout(LogoutArgs),
    /// Not supported: the session comes from `auth login` (always a usage error).
    SetCredential(SetCredentialArgs),
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// Print the resolved config file path.
    Path,
    /// Show the effective configuration.
    Show,
    /// Set a config key (`username`, `default_cloud`).
    Set { key: String, value: String },
    /// Remove a config key.
    Unset { key: String },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum LedState {
    On,
    Off,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PowerAction {
    On,
    Off,
}
