use serde_json::json;

use crate::api::device_client::DeviceClient;
use crate::error::AppError;
use crate::models::device_info::DeviceInfo;
use crate::models::device_type::DeviceType;

const LIGHTING_SERVICE: &str = "smartlife.iot.smartbulb.lightingservice";

pub struct Device {
    client: DeviceClient,
    pub device_id: String,
    pub info: DeviceInfo,
    pub device_type: DeviceType,
    pub child_id: Option<String>,
}

impl Device {
    pub fn new(
        client: DeviceClient,
        device_id: String,
        info: DeviceInfo,
        device_type: DeviceType,
        child_id: Option<String>,
    ) -> Self {
        Self {
            client,
            device_id,
            info,
            device_type,
            child_id,
        }
    }

    pub fn alias(&self) -> &str {
        self.info.alias_or_name()
    }

    /// Build and send a passthrough request, handling child device context.
    async fn passthrough(
        &self,
        request_type: &str,
        sub_request_type: &str,
        request: serde_json::Value,
    ) -> Result<Option<serde_json::Value>, AppError> {
        let mut request_data = json!({
            request_type: {
                sub_request_type: request,
            }
        });

        // Inject child context if this is a child device
        if let Some(ref child_id) = self.child_id {
            request_data["context"] = json!({
                "child_ids": [child_id]
            });
        }

        let response = self
            .client
            .passthrough(&self.device_id, request_data)
            .await?;

        if let Some(response_data) = response {
            // Navigate to the sub-request response
            if let Some(request_response) = response_data.get(request_type) {
                if let Some(sub_response) = request_response.get(sub_request_type) {
                    // For child devices, find the matching child in the response
                    if let Some(ref child_id) = self.child_id {
                        if let Some(children) = sub_response.get("children") {
                            if let Some(arr) = children.as_array() {
                                for child in arr {
                                    if child.get("id").and_then(|v| v.as_str()) == Some(child_id) {
                                        return Ok(Some(child.clone()));
                                    }
                                }
                            }
                        }
                    }
                    return Ok(Some(sub_response.clone()));
                }
            }
        }

        Ok(None)
    }

    // -- Power operations --

    pub async fn power_on(&self) -> Result<Option<serde_json::Value>, AppError> {
        if self.device_type.is_light() {
            self.passthrough(
                LIGHTING_SERVICE,
                "transition_light_state",
                json!({"on_off": 1}),
            )
            .await
        } else {
            self.passthrough("system", "set_relay_state", json!({"state": 1}))
                .await
        }
    }

    pub async fn power_off(&self) -> Result<Option<serde_json::Value>, AppError> {
        if self.device_type.is_light() {
            self.passthrough(
                LIGHTING_SERVICE,
                "transition_light_state",
                json!({"on_off": 0}),
            )
            .await
        } else {
            self.passthrough("system", "set_relay_state", json!({"state": 0}))
                .await
        }
    }

    pub async fn toggle(&self) -> Result<Option<serde_json::Value>, AppError> {
        match self.is_on().await? {
            Some(true) => self.power_off().await,
            Some(false) => self.power_on().await,
            None => Err(AppError::Api {
                message: "Could not determine device power state".into(),
                error_code: None,
            }),
        }
    }

    pub async fn is_on(&self) -> Result<Option<bool>, AppError> {
        let sys_info = self.get_sys_info().await?;
        if let Some(info) = sys_info {
            if self.device_type.is_light() {
                // Light devices use light_state.on_off
                if let Some(light_state) = info.get("light_state") {
                    return Ok(light_state
                        .get("on_off")
                        .and_then(|v| v.as_i64())
                        .map(|v| v == 1));
                }
            }
            if self.child_id.is_some() {
                return Ok(info.get("state").and_then(|v| v.as_i64()).map(|v| v == 1));
            }
            return Ok(info
                .get("relay_state")
                .and_then(|v| v.as_i64())
                .map(|v| v == 1));
        }
        Ok(None)
    }

    // -- System info --

    pub async fn get_sys_info(&self) -> Result<Option<serde_json::Value>, AppError> {
        self.passthrough("system", "get_sysinfo", json!(null)).await
    }

    // -- LED --

    pub async fn set_led_state(&self, on: bool) -> Result<Option<serde_json::Value>, AppError> {
        // API contract: "led_off" where 0 = LED on, 1 = LED off
        let led_off_state = if on { 0 } else { 1 };
        self.passthrough("system", "set_led_off", json!({"off": led_off_state}))
            .await
    }

    // -- Energy monitoring --

    pub async fn get_power_usage_realtime(&self) -> Result<Option<serde_json::Value>, AppError> {
        if !self.device_type.has_emeter() {
            return Err(AppError::UnsupportedOperation(format!(
                "{} does not support energy monitoring",
                self.device_type.display_name()
            )));
        }
        self.passthrough("emeter", "get_realtime", json!(null))
            .await
    }

    pub async fn get_power_usage_day(
        &self,
        year: i32,
        month: u32,
    ) -> Result<Option<serde_json::Value>, AppError> {
        if !self.device_type.has_emeter() {
            return Err(AppError::UnsupportedOperation(format!(
                "{} does not support energy monitoring",
                self.device_type.display_name()
            )));
        }
        self.passthrough(
            "emeter",
            "get_daystat",
            json!({"year": year, "month": month}),
        )
        .await
    }

    pub async fn get_power_usage_month(
        &self,
        year: i32,
    ) -> Result<Option<serde_json::Value>, AppError> {
        if !self.device_type.has_emeter() {
            return Err(AppError::UnsupportedOperation(format!(
                "{} does not support energy monitoring",
                self.device_type.display_name()
            )));
        }
        self.passthrough("emeter", "get_monthstat", json!({"year": year}))
            .await
    }

    // -- Light operations --

    pub async fn get_light_state(&self) -> Result<Option<serde_json::Value>, AppError> {
        if !self.device_type.is_light() {
            return Err(AppError::UnsupportedOperation(format!(
                "{} is not a light device",
                self.device_type.display_name()
            )));
        }
        self.passthrough(LIGHTING_SERVICE, "get_light_state", json!({}))
            .await
    }

    pub async fn set_light_state(
        &self,
        on_off: Option<i32>,
        brightness: Option<u8>,
        hue: Option<u16>,
        saturation: Option<u8>,
        color_temp: Option<u16>,
        transition_period: Option<u32>,
    ) -> Result<Option<serde_json::Value>, AppError> {
        if !self.device_type.is_light() {
            return Err(AppError::UnsupportedOperation(format!(
                "{} is not a light device",
                self.device_type.display_name()
            )));
        }
        let mut state = serde_json::Map::new();
        if let Some(v) = on_off {
            state.insert("on_off".into(), json!(v));
        }
        if let Some(v) = brightness {
            state.insert("brightness".into(), json!(v));
        }
        if let Some(v) = hue {
            state.insert("hue".into(), json!(v));
        }
        if let Some(v) = saturation {
            state.insert("saturation".into(), json!(v));
        }
        if let Some(v) = color_temp {
            state.insert("color_temp".into(), json!(v));
        }
        if let Some(v) = transition_period {
            state.insert("transition_period".into(), json!(v));
        }
        self.passthrough(
            LIGHTING_SERVICE,
            "transition_light_state",
            serde_json::Value::Object(state),
        )
        .await
    }

    pub async fn set_brightness(
        &self,
        brightness: u8,
    ) -> Result<Option<serde_json::Value>, AppError> {
        self.set_light_state(Some(1), Some(brightness), None, None, None, None)
            .await
    }

    pub async fn set_color(
        &self,
        hue: u16,
        saturation: u8,
        brightness: Option<u8>,
    ) -> Result<Option<serde_json::Value>, AppError> {
        self.set_light_state(
            Some(1),
            brightness,
            Some(hue),
            Some(saturation),
            Some(0),
            None,
        )
        .await
    }

    pub async fn set_color_temp(
        &self,
        color_temp: u16,
        brightness: Option<u8>,
    ) -> Result<Option<serde_json::Value>, AppError> {
        self.set_light_state(Some(1), brightness, None, None, Some(color_temp), None)
            .await
    }

    // -- Schedules --

    pub async fn get_schedule_rules(&self) -> Result<Option<serde_json::Value>, AppError> {
        self.passthrough("schedule", "get_rules", json!({})).await
    }

    pub async fn add_schedule_rule(
        &self,
        rule: serde_json::Value,
    ) -> Result<Option<serde_json::Value>, AppError> {
        self.passthrough("schedule", "add_rule", rule).await
    }

    pub async fn edit_schedule_rule(
        &self,
        rule: serde_json::Value,
    ) -> Result<Option<serde_json::Value>, AppError> {
        self.passthrough("schedule", "edit_rule", rule).await
    }

    pub async fn delete_schedule_rule(
        &self,
        rule_id: &str,
    ) -> Result<Option<serde_json::Value>, AppError> {
        self.passthrough("schedule", "delete_rule", json!({"id": rule_id}))
            .await
    }

    pub async fn delete_all_schedule_rules(&self) -> Result<Option<serde_json::Value>, AppError> {
        self.passthrough("schedule", "delete_all_rules", json!(null))
            .await
    }

    // -- Network/Time info --

    pub async fn get_net_info(&self) -> Result<Option<serde_json::Value>, AppError> {
        self.passthrough("netif", "get_stainfo", json!(null)).await
    }

    pub async fn get_time(&self) -> Result<Option<serde_json::Value>, AppError> {
        self.passthrough("time", "get_time", json!({})).await
    }

    pub async fn get_timezone(&self) -> Result<Option<serde_json::Value>, AppError> {
        self.passthrough("time", "get_timezone", json!({})).await
    }

    // -- Children --

    pub async fn get_children(&self) -> Result<Vec<ChildInfo>, AppError> {
        if !self.device_type.has_children() {
            return Ok(vec![]);
        }

        let sys_info = self.get_sys_info().await?;
        let mut children = Vec::new();

        if let Some(info) = sys_info {
            if let Some(children_arr) = info.get("children").and_then(|v| v.as_array()) {
                for child in children_arr {
                    let child_id = child
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let alias = child
                        .get("alias")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let state = child
                        .get("state")
                        .and_then(|v| v.as_i64())
                        .map(|v| v as i32);
                    children.push(ChildInfo {
                        id: child_id,
                        alias,
                        state,
                    });
                }
            }
        }

        Ok(children)
    }
}

#[derive(Debug, Clone)]
pub struct ChildInfo {
    pub id: String,
    pub alias: String,
    pub state: Option<i32>,
}
