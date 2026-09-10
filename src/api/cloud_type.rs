use serde::{Deserialize, Serialize};

/// Which TP-Link cloud ecosystem a device belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum CloudType {
    Kasa,
    Tapo,
}

impl CloudType {
    /// Parse a user-supplied cloud name (case-insensitive).
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "kasa" => Ok(CloudType::Kasa),
            "tapo" => Ok(CloudType::Tapo),
            other => Err(format!("unknown cloud `{other}` (expected kasa or tapo)")),
        }
    }

    pub fn host(&self) -> &'static str {
        match self {
            CloudType::Kasa => "https://n-wap.tplinkcloud.com",
            CloudType::Tapo => "https://n-wap.i.tplinkcloud.com",
        }
    }

    /// App-level access key extracted from the Android APK.
    /// These identify the app to the API server, not the user.
    /// They are identical across all installations and are public knowledge.
    pub fn access_key(&self) -> &'static str {
        match self {
            CloudType::Kasa => "e37525375f8845999bcc56d5e6faa76d",
            CloudType::Tapo => "4d11b6b9d5ea4d19a829adbb9714b057",
        }
    }

    /// App-level secret key extracted from the Android APK.
    /// Used for HMAC-SHA1 request signing. Not a user secret.
    pub fn secret_key(&self) -> &'static str {
        match self {
            CloudType::Kasa => "314bc6700b3140ca80bc655e527cb062",
            CloudType::Tapo => "6ed7d97f3e73467f8a5bab90b577ba4c",
        }
    }

    pub fn app_type(&self) -> &'static str {
        match self {
            CloudType::Kasa => "Kasa_Android_Mix",
            CloudType::Tapo => "TP-Link_Tapo_Android",
        }
    }

    pub fn app_version(&self) -> &'static str {
        "3.4.451"
    }

    pub fn passthrough_path(&self) -> &'static str {
        match self {
            CloudType::Kasa => "/",
            CloudType::Tapo => "/api/v2/common/passthrough",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            CloudType::Kasa => "kasa",
            CloudType::Tapo => "tapo",
        }
    }
}

impl std::fmt::Display for CloudType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_is_case_insensitive_and_rejects_unknowns() {
        assert_eq!(CloudType::parse("Kasa"), Ok(CloudType::Kasa));
        assert_eq!(CloudType::parse("TAPO"), Ok(CloudType::Tapo));
        assert!(CloudType::parse("hue").is_err());
    }

    #[test]
    fn serializes_lowercase() {
        assert_eq!(serde_json::to_value(CloudType::Tapo).unwrap(), "tapo");
        let back: CloudType = serde_json::from_value(serde_json::json!("kasa")).unwrap();
        assert_eq!(back, CloudType::Kasa);
    }
}
