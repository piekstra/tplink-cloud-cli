use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceNetInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_type: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rssi: Option<i32>,
}

impl DeviceNetInfo {
    pub fn from_json(data: &serde_json::Value) -> Self {
        Self {
            ssid: data
                .get("ssid")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            key_type: data
                .get("key_type")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32),
            rssi: data.get("rssi").and_then(|v| v.as_i64()).map(|v| v as i32),
        }
    }
}
