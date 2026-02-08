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
