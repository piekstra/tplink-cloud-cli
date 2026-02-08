use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LightState {
    pub on_off: Option<i32>,
    pub mode: Option<String>,
    pub hue: Option<u16>,
    pub saturation: Option<u8>,
    pub color_temp: Option<u16>,
    pub brightness: Option<u8>,
}

impl LightState {
    pub fn from_json(data: &serde_json::Value) -> Self {
        Self {
            on_off: data
                .get("on_off")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32),
            mode: data
                .get("mode")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            hue: data.get("hue").and_then(|v| v.as_u64()).map(|v| v as u16),
            saturation: data
                .get("saturation")
                .and_then(|v| v.as_u64())
                .map(|v| v as u8),
            color_temp: data
                .get("color_temp")
                .and_then(|v| v.as_u64())
                .map(|v| v as u16),
            brightness: data
                .get("brightness")
                .and_then(|v| v.as_u64())
                .map(|v| v as u8),
        }
    }
}
