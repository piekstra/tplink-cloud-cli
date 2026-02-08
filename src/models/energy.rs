use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CurrentPower {
    pub voltage_mv: Option<f64>,
    pub current_ma: Option<f64>,
    pub power_mw: Option<f64>,
    pub total_wh: Option<f64>,
}

impl CurrentPower {
    pub fn from_json(data: &serde_json::Value) -> Self {
        Self {
            voltage_mv: data
                .get("voltage_mv")
                .or_else(|| data.get("voltage"))
                .and_then(|v| v.as_f64()),
            current_ma: data
                .get("current_ma")
                .or_else(|| data.get("current"))
                .and_then(|v| v.as_f64()),
            power_mw: data
                .get("power_mw")
                .or_else(|| data.get("power"))
                .and_then(|v| v.as_f64()),
            total_wh: data
                .get("total_wh")
                .or_else(|| data.get("total"))
                .and_then(|v| v.as_f64()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DayPowerSummary {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub energy_wh: Option<f64>,
}

impl DayPowerSummary {
    pub fn from_json(data: &serde_json::Value) -> Self {
        Self {
            year: data.get("year").and_then(|v| v.as_i64()).map(|v| v as i32),
            month: data.get("month").and_then(|v| v.as_i64()).map(|v| v as u32),
            day: data.get("day").and_then(|v| v.as_i64()).map(|v| v as u32),
            energy_wh: data
                .get("energy_wh")
                .or_else(|| data.get("energy"))
                .and_then(|v| v.as_f64()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MonthPowerSummary {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub energy_wh: Option<f64>,
}

impl MonthPowerSummary {
    pub fn from_json(data: &serde_json::Value) -> Self {
        Self {
            year: data.get("year").and_then(|v| v.as_i64()).map(|v| v as i32),
            month: data.get("month").and_then(|v| v.as_i64()).map(|v| v as u32),
            energy_wh: data
                .get("energy_wh")
                .or_else(|| data.get("energy"))
                .and_then(|v| v.as_f64()),
        }
    }
}
