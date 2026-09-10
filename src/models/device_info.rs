use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::api::cloud_type::CloudType;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub device_type: Option<String>,
    pub role: Option<i32>,
    pub fw_ver: Option<String>,
    pub app_server_url: Option<String>,
    pub device_region: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub device_hw_ver: Option<String>,
    pub alias: Option<String>,
    pub device_mac: Option<String>,
    pub oem_id: Option<String>,
    pub device_model: Option<String>,
    pub hw_id: Option<String>,
    pub fw_id: Option<String>,
    pub is_same_region: Option<bool>,
    pub status: Option<i32>,

    /// Which cloud this device was discovered from (not from API, set by CLI).
    #[serde(skip_deserializing)]
    pub cloud_type: Option<CloudType>,
}

impl DeviceInfo {
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }

    /// A device as one cloud listed it. This is where a name enters the
    /// program, so it is also where the Tapo cloud's encoding is undone:
    /// Tapo hands aliases through base64 (its firmware reports them that
    /// way), Kasa hands them through as typed.
    pub fn from_cloud(value: &serde_json::Value, cloud: CloudType) -> Option<Self> {
        let mut info = Self::from_json(value)?;
        info.cloud_type = Some(cloud);
        if cloud == CloudType::Tapo {
            info.alias = info.alias.as_deref().map(decode_encoded_name);
        }
        Some(info)
    }

    pub fn alias_or_name(&self) -> &str {
        self.alias
            .as_deref()
            .or(self.device_name.as_deref())
            .unwrap_or("Unknown")
    }

    pub fn model(&self) -> &str {
        self.device_model.as_deref().unwrap_or("Unknown")
    }

    pub fn id(&self) -> &str {
        self.device_id.as_deref().unwrap_or("")
    }
}

/// A device name as Tapo reports it: the firmware base64-encodes names and
/// the clouds pass some through as-is, so a value that is valid base64 of
/// clean UTF-8 text is taken as encoded; anything else is the name.
pub fn decode_encoded_name(raw: &str) -> String {
    if raw.len() % 4 == 0
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
    {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(raw) {
            if let Ok(s) = String::from_utf8(bytes) {
                if !s.is_empty() && s.chars().all(|c| !c.is_control()) {
                    return s;
                }
            }
        }
    }
    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encoded_names_decode_and_plain_names_survive() {
        assert_eq!(decode_encoded_name("T2ZmaWNlIExhbXA="), "Office Lamp");
        assert_eq!(
            decode_encoded_name("RnJvbnQgRG9vciBMb2Nr"),
            "Front Door Lock"
        );
        assert_eq!(decode_encoded_name("Office Lamp"), "Office Lamp");
        // Four base64 characters that do not decode to text stay as typed.
        assert_eq!(decode_encoded_name("Lamp"), "Lamp");
        assert_eq!(decode_encoded_name("Desk"), "Desk");
        assert_eq!(decode_encoded_name(""), "");
    }

    #[test]
    fn the_tapo_alias_is_decoded_at_the_boundary_and_kasa_is_not() {
        let listed =
            json!({"deviceId": "d1", "alias": "RnJvbnQgRG9vciBMb2Nr", "deviceModel": "L530"});
        let tapo = DeviceInfo::from_cloud(&listed, CloudType::Tapo).unwrap();
        assert_eq!(tapo.alias_or_name(), "Front Door Lock");
        assert_eq!(tapo.cloud_type, Some(CloudType::Tapo));
        let kasa = DeviceInfo::from_cloud(&listed, CloudType::Kasa).unwrap();
        assert_eq!(
            kasa.alias_or_name(),
            "RnJvbnQgRG9vciBMb2Nr",
            "Kasa names are as typed"
        );
        let plain = json!({"deviceId": "d2", "alias": "Porch Light"});
        assert_eq!(
            DeviceInfo::from_cloud(&plain, CloudType::Tapo)
                .unwrap()
                .alias_or_name(),
            "Porch Light"
        );
    }
}
