//! `tplc` — TP-Link Kasa and Tapo devices from the terminal, over the cloud
//! API. Conforms to piekstra-cli/1: `--json` everywhere (one schema-tagged
//! DTO per command), the family exit codes, keychain-only secrets.
//!
//! Layering: `api/` and `models/` are the vendor client (async, `AppError`);
//! `session.rs` is the keychain; `cli/` is the clap tree and one handler
//! module per command group, all in `CliError`; [`run`] dispatches.

pub mod api;
pub mod cli;
pub mod config;
pub mod error;
pub mod models;
pub mod resolve;
pub mod session;

use clap::CommandFactory;
use pk_cli_config::ConfigStore;
use pk_cli_core::info::{AuthInfo, CliInfo};
use pk_cli_core::{output, CliError};
use pk_cli_selfupdate::Updater;

use cli::emit::Ctx;
use cli::{AuthCmd, Cli, Command};
use config::Config;
use session::Sessions;

pub const BIN: &str = session::BIN;
const REPO: &str = "piekstra/tplink-cloud-cli";

/// Run one invocation and return the process exit code. Errors are the
/// caller's to report (`output::fail`); an `Ok` code other than 0 means the
/// command already emitted its own outcome (a login parked for MFA).
pub fn run(cli: &Cli) -> Result<i32, CliError> {
    if cli.table && !cli.common.quiet {
        eprintln!("note: --table is deprecated and ignored; text output is the default (use --json for JSON)");
    }
    let store = ConfigStore::new(BIN).with_override(cli.config.clone());

    // Offline commands first: nothing here may touch the keychain or the
    // vendor API, and self-update's blocking HTTP must run outside the
    // async runtime built below.
    match &cli.command {
        Command::Config(cmd) => return cli::config::run(cli.common.json, cmd, &store).map(|()| 0),
        Command::SelfUpdate(args) => {
            return Updater {
                repo: REPO.into(),
                binary: BIN.into(),
                target: env!("BUILD_TARGET").into(),
                current: env!("CARGO_PKG_VERSION").into(),
            }
            .run(args, cli.common.json, cli.common.quiet)
            .map(|()| 0)
        }
        Command::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), BIN, &mut std::io::stdout());
            return Ok(0);
        }
        Command::Info { cmd: None } => {
            output::json(&serde_json::to_value(info()).unwrap_or_default());
            return Ok(0);
        }
        Command::Auth(AuthCmd::SetCredential(args)) => {
            return cli::auth::set_credential(args).map(|()| 0)
        }
        _ => {}
    }

    // Argument validation that needs no I/O, so bad input never reaches the
    // keychain (SPEC §1.5: exit 2 first).
    let api_params = match &cli.command {
        Command::Api(args) => cli::api::validate(args)?,
        Command::Schedule(cmd) => {
            cli::schedule::validate(cmd)?;
            None
        }
        Command::Rooms(cmd) => {
            cli::rooms::gate(cmd, cli.common.interactive())?;
            None
        }
        _ => None,
    };

    let cfg: Config = store.load()?;
    let sessions = Sessions::new();
    let ctx = Ctx {
        json: cli.common.json,
        verbose: cli.common.verbose,
        quiet: cli.common.quiet,
        interactive: cli.common.interactive(),
        store: &store,
        sessions: &sessions,
        cfg,
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CliError::Other(format!("starting async runtime: {e}")))?;
    rt.block_on(dispatch(cli, &ctx, api_params))
}

async fn dispatch(
    cli: &Cli,
    ctx: &Ctx<'_>,
    api_params: Option<serde_json::Value>,
) -> Result<i32, CliError> {
    match &cli.command {
        Command::Auth(AuthCmd::Login(args)) | Command::Login(args) => {
            return cli::auth::login(ctx, args).await
        }
        Command::Auth(AuthCmd::Status) | Command::Status => cli::auth::status(ctx)?,
        Command::Auth(AuthCmd::Logout(args)) | Command::Logout(args) => {
            cli::auth::logout(ctx, args)?
        }
        Command::Devices(cmd) => cli::devices::handle(ctx, cmd).await?,
        Command::Power(cmd) => cli::power::handle(ctx, cmd).await?,
        Command::Energy(cmd) => cli::energy::handle(ctx, cmd).await?,
        Command::Light(cmd) => cli::light::handle(ctx, cmd).await?,
        Command::Schedule(cmd) => cli::schedule::handle(ctx, cmd).await?,
        Command::Info { cmd: Some(cmd) } => cli::info::handle(ctx, cmd).await?,
        Command::Rooms(cmd) => cli::rooms::handle(ctx, cmd).await?,
        Command::Groups(cmd) => cli::groups::handle(ctx, cmd).await?,
        Command::Api(args) => cli::api::handle(ctx, args, api_params).await?,
        Command::Led { state, device } => cli::led::handle(ctx, *state, device).await?,
        Command::Auth(AuthCmd::SetCredential(_))
        | Command::Config(_)
        | Command::SelfUpdate(_)
        | Command::Completions { .. }
        | Command::Info { cmd: None } => unreachable!("handled before the runtime"),
    }
    Ok(0)
}

/// The `cli-info/v1` discovery document (SPEC §1.6).
pub fn info() -> CliInfo {
    CliInfo::new(
        BIN,
        env!("CARGO_PKG_VERSION"),
        &format!("https://github.com/{REPO}"),
        AuthInfo {
            required: true,
            method: "password".into(),
            login_hint: Some(format!("{BIN} auth login")),
        },
        &[
            "devices", "power", "energy", "light", "schedule", "info", "led", "rooms", "groups",
            "api",
        ],
    )
    .with_profiles(&[SMART_HOME_PROFILE])
}

/// The documented-only `smart-home/v1` profile (DESIGN.md §1.8): `rooms
/// devices` and `groups devices` emit its `device-rooms/v1` shape.
pub const SMART_HOME_PROFILE: &str = "smart-home/v1";
