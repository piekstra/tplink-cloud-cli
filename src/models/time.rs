use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceTime {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub mday: Option<u32>,
    pub hour: Option<u32>,
    pub min: Option<u32>,
    pub sec: Option<u32>,
}

impl DeviceTime {
    pub fn from_json(data: &serde_json::Value) -> Self {
        Self {
            year: data.get("year").and_then(|v| v.as_i64()).map(|v| v as i32),
            month: data.get("month").and_then(|v| v.as_u64()).map(|v| v as u32),
            mday: data.get("mday").and_then(|v| v.as_u64()).map(|v| v as u32),
            hour: data.get("hour").and_then(|v| v.as_u64()).map(|v| v as u32),
            min: data.get("min").and_then(|v| v.as_u64()).map(|v| v as u32),
            sec: data.get("sec").and_then(|v| v.as_u64()).map(|v| v as u32),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceTimezone {
    pub index: Option<i32>,
}

impl DeviceTimezone {
    pub fn from_json(data: &serde_json::Value) -> Self {
        Self {
            index: data.get("index").and_then(|v| v.as_i64()).map(|v| v as i32),
        }
    }
}
