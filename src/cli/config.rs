//! `config path|show|set|unset` over `pk_cli_config::ConfigStore`. `show`,
//! `set` and `unset` all emit `config/v1` — the effective configuration —
//! so a scripted `set` has a document to parse like every other command.

use pk_cli_config::ConfigStore;
use pk_cli_core::output::emit_one;
use pk_cli_core::CliError;

use super::ConfigCmd;
use crate::config::{Config, KEYS};

fn emit_config(json: bool, cfg: &Config) {
    let v = serde_json::to_value(cfg).unwrap_or_default();
    if !json && v.as_object().is_some_and(|m| m.is_empty()) {
        println!("(no settings; keys: {})", KEYS.join(", "));
    } else {
        emit_one(json, "config", v);
    }
}

pub fn run(json: bool, cmd: &ConfigCmd, store: &ConfigStore) -> Result<(), CliError> {
    match cmd {
        ConfigCmd::Path => {
            println!("{}", store.path()?.display());
            Ok(())
        }
        ConfigCmd::Show => {
            let cfg: Config = store.load()?;
            emit_config(json, &cfg);
            Ok(())
        }
        ConfigCmd::Set { key, value } => {
            let mut cfg: Config = store.load()?;
            cfg.set(key, value).map_err(CliError::Usage)?;
            store.save(&cfg)?;
            emit_config(json, &cfg);
            Ok(())
        }
        ConfigCmd::Unset { key } => {
            let mut cfg: Config = store.load()?;
            cfg.unset(key).map_err(CliError::Usage)?;
            store.save(&cfg)?;
            emit_config(json, &cfg);
            Ok(())
        }
    }
}
