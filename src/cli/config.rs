//! `config path|show|set|unset` over `pk_cli_config::ConfigStore`.

use pk_cli_config::ConfigStore;
use pk_cli_core::{output, CliError};

use super::ConfigCmd;
use crate::config::Config;

pub fn run(json: bool, cmd: &ConfigCmd, store: &ConfigStore) -> Result<(), CliError> {
    match cmd {
        ConfigCmd::Path => {
            println!("{}", store.path()?.display());
            Ok(())
        }
        ConfigCmd::Show => {
            let cfg: Config = store.load()?;
            let v = serde_json::to_value(&cfg).unwrap_or_default();
            if json {
                output::json(&v);
            } else if v.as_object().is_some_and(|m| m.is_empty()) {
                println!("(no settings; keys: {})", crate::config::KEYS.join(", "));
            } else {
                output::render(&v);
            }
            Ok(())
        }
        ConfigCmd::Set { key, value } => {
            let mut cfg: Config = store.load()?;
            cfg.set(key, value).map_err(CliError::Usage)?;
            store.save(&cfg)
        }
        ConfigCmd::Unset { key } => {
            let mut cfg: Config = store.load()?;
            cfg.unset(key).map_err(CliError::Usage)?;
            store.save(&cfg)
        }
    }
}
