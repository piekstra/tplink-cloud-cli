//! Non-secret settings (`~/.config/tplc/config.json`, or `--config` /
//! `$TPLC_CONFIG`). Session tokens never live here — they are keychain-only
//! (`piekstra.tplc`, see `session.rs`).

use serde::{Deserialize, Serialize};

use crate::api::cloud_type::CloudType;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// TP-Link account email; the default identity for `auth login`.
    /// Written by a successful login.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Which cloud `api` calls when `--cloud` is not given: `kasa` (default)
    /// or `tapo`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_cloud: Option<CloudType>,
}

pub const KEYS: &[&str] = &["username", "default_cloud"];

impl Config {
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "username" => self.username = Some(value.to_string()),
            "default_cloud" => self.default_cloud = Some(CloudType::parse(value)?),
            other => return Err(unknown(other)),
        }
        Ok(())
    }

    pub fn unset(&mut self, key: &str) -> Result<(), String> {
        match key {
            "username" => self.username = None,
            "default_cloud" => self.default_cloud = None,
            other => return Err(unknown(other)),
        }
        Ok(())
    }
}

fn unknown(key: &str) -> String {
    format!("unknown config key `{key}` (known: {})", KEYS.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_validates_keys_and_cloud_values() {
        let mut cfg = Config::default();
        cfg.set("default_cloud", "tapo").unwrap();
        assert_eq!(cfg.default_cloud, Some(CloudType::Tapo));
        assert!(cfg
            .set("default_cloud", "hue")
            .unwrap_err()
            .contains("kasa"));
        assert!(cfg
            .set("bogus", "x")
            .unwrap_err()
            .contains("unknown config key"));
        cfg.unset("default_cloud").unwrap();
        assert!(cfg.default_cloud.is_none());
        assert!(cfg.unset("bogus").is_err());
    }

    #[test]
    fn empty_config_serializes_to_an_empty_object() {
        let v = serde_json::to_value(Config::default()).unwrap();
        assert_eq!(v, serde_json::json!({}));
    }
}
