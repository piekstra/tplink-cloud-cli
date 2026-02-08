use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartOption {
    Time = 0,
    Sunrise = 1,
    Sunset = 2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleRule {
    pub id: Option<String>,
    pub name: Option<String>,
    pub enable: Option<i32>,
    pub wday: Option<Vec<i32>>,
    pub stime_opt: Option<i32>,
    pub soffset: Option<i32>,
    pub smin: Option<i32>,
    pub sact: Option<i32>,
    pub etime_opt: Option<i32>,
    pub eoffset: Option<i32>,
    pub emin: Option<i32>,
    pub eact: Option<i32>,
    pub repeat: Option<i32>,
    pub year: Option<i32>,
    pub month: Option<i32>,
    pub day: Option<i32>,
}

impl ScheduleRule {
    pub fn from_json(data: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(data.clone()).ok()
    }
}

/// Builder for constructing schedule rules.
pub struct ScheduleRuleBuilder {
    action: Option<bool>,
    name: Option<String>,
    enabled: bool,
    time_opt: StartOption,
    minutes: Option<i32>,
    wday: Option<Vec<i32>>,
    repeat: bool,
    year: Option<i32>,
    month: Option<i32>,
    day: Option<i32>,
}

impl Default for ScheduleRuleBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ScheduleRuleBuilder {
    pub fn new() -> Self {
        Self {
            action: None,
            name: None,
            enabled: true,
            time_opt: StartOption::Time,
            minutes: None,
            wday: None,
            repeat: true,
            year: None,
            month: None,
            day: None,
        }
    }

    pub fn with_action(mut self, turn_on: bool) -> Self {
        self.action = Some(turn_on);
        self
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_time(mut self, hour: u32, minute: u32) -> Self {
        self.time_opt = StartOption::Time;
        self.minutes = Some((hour * 60 + minute) as i32);
        self
    }

    pub fn with_sunrise(mut self) -> Self {
        self.time_opt = StartOption::Sunrise;
        self.minutes = Some(0);
        self
    }

    pub fn with_sunset(mut self) -> Self {
        self.time_opt = StartOption::Sunset;
        self.minutes = Some(0);
        self
    }

    /// Set days of week. Array of 7 values [Sun, Mon, Tue, Wed, Thu, Fri, Sat].
    /// 1 = active, 0 = inactive.
    pub fn with_days(mut self, wday: Vec<i32>) -> Self {
        self.wday = Some(wday);
        self.repeat = true;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn build(self) -> Result<serde_json::Value, AppError> {
        let action = self.action.ok_or_else(|| {
            AppError::InvalidInput("Schedule rule requires an action (on/off)".into())
        })?;

        let sact = if action { 1 } else { 0 };
        let smin = self.minutes.unwrap_or(0);

        let mut rule = serde_json::json!({
            "enable": if self.enabled { 1 } else { 0 },
            "sact": sact,
            "stime_opt": self.time_opt as i32,
            "smin": smin,
            "soffset": 0,
            "etime_opt": -1,
            "emin": 0,
            "eoffset": 0,
            "eact": -1,
            "repeat": if self.repeat { 1 } else { 0 },
        });

        if let Some(name) = &self.name {
            rule["name"] = serde_json::json!(name);
        }

        if let Some(wday) = &self.wday {
            rule["wday"] = serde_json::json!(wday);
        } else {
            // Default: all days
            rule["wday"] = serde_json::json!([1, 1, 1, 1, 1, 1, 1]);
        }

        if !self.repeat {
            if let Some(year) = self.year {
                rule["year"] = serde_json::json!(year);
            }
            if let Some(month) = self.month {
                rule["month"] = serde_json::json!(month);
            }
            if let Some(day) = self.day {
                rule["day"] = serde_json::json!(day);
            }
        }

        Ok(rule)
    }
}

/// Parse day name abbreviations to wday array indices.
/// Returns [Sun, Mon, Tue, Wed, Thu, Fri, Sat].
pub fn parse_days(days: &[String]) -> Result<Vec<i32>, AppError> {
    let mut wday = vec![0; 7];
    for day in days {
        let idx = match day.to_lowercase().as_str() {
            "sun" | "sunday" => 0,
            "mon" | "monday" => 1,
            "tue" | "tuesday" => 2,
            "wed" | "wednesday" => 3,
            "thu" | "thursday" => 4,
            "fri" | "friday" => 5,
            "sat" | "saturday" => 6,
            other => {
                return Err(AppError::InvalidInput(format!(
                    "Invalid day: '{}'. Use: sun, mon, tue, wed, thu, fri, sat",
                    other
                )));
            }
        };
        wday[idx] = 1;
    }
    Ok(wday)
}

/// Parse time string "HH:MM" to (hour, minute).
pub fn parse_time(time_str: &str) -> Result<(u32, u32), AppError> {
    let parts: Vec<&str> = time_str.split(':').collect();
    if parts.len() != 2 {
        return Err(AppError::InvalidInput(format!(
            "Invalid time format '{}'. Use HH:MM",
            time_str
        )));
    }
    let hour: u32 = parts[0]
        .parse()
        .map_err(|_| AppError::InvalidInput(format!("Invalid hour in '{}'", time_str)))?;
    let minute: u32 = parts[1]
        .parse()
        .map_err(|_| AppError::InvalidInput(format!("Invalid minute in '{}'", time_str)))?;
    if hour > 23 || minute > 59 {
        return Err(AppError::InvalidInput(format!(
            "Time '{}' out of range (00:00-23:59)",
            time_str
        )));
    }
    Ok((hour, minute))
}
