use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;

use super::client::{client_builder, ensure_provider_url};
use super::cloud_type::CloudType;
use super::errors::*;
use super::response::ApiResponse;
use super::signing::get_signing_headers;
use crate::error::AppError;

/// How long a command in a terminal waits for the cloud to reach a device (docs/api.md).
pub const PASSTHROUGH_TIMEOUT: Duration = Duration::from_secs(600);

pub struct DeviceClient {
    client: reqwest::Client,
    host: String,
    cloud_type: CloudType,
    query_params: HashMap<String, String>,
    verbose: bool,
}

impl DeviceClient {
    pub fn new(
        host: &str,
        token: &str,
        term_id: &str,
        verbose: bool,
        cloud_type: CloudType,
    ) -> Result<Self, AppError> {
        Self::with_timeout(
            host,
            token,
            term_id,
            verbose,
            cloud_type,
            PASSTHROUGH_TIMEOUT,
        )
    }

    /// As [`new`](Self::new), waiting no longer than `timeout` for each request.
    pub fn with_timeout(
        host: &str,
        token: &str,
        term_id: &str,
        verbose: bool,
        cloud_type: CloudType,
        timeout: Duration,
    ) -> Result<Self, AppError> {
        ensure_provider_url(host)?;
        let client = client_builder()?
            .user_agent("Dalvik/2.1.0 (Linux; U; Android 14; Pixel Build/UP1A)")
            .timeout(timeout)
            .build()?;

        let mut query_params = HashMap::new();
        query_params.insert("appName".into(), cloud_type.app_type().into());
        query_params.insert("appVer".into(), cloud_type.app_version().into());
        query_params.insert("netType".into(), "wifi".into());
        query_params.insert("termID".into(), term_id.into());
        query_params.insert("ospf".into(), "Android 14".into());
        query_params.insert("brand".into(), "TPLINK".into());
        query_params.insert("locale".into(), "en_US".into());
        query_params.insert("model".into(), "Pixel".into());
        query_params.insert("termName".into(), "Pixel".into());
        query_params.insert("termMeta".into(), "Pixel".into());
        query_params.insert("token".into(), token.into());

        Ok(Self {
            client,
            host: host.to_string(),
            cloud_type,
            query_params,
            verbose,
        })
    }

    /// Send a passthrough command to a device and return the parsed response data.
    pub async fn passthrough(
        &self,
        device_id: &str,
        request_data: serde_json::Value,
    ) -> Result<Option<serde_json::Value>, AppError> {
        let request_data_str = serde_json::to_string(&request_data)?;

        // Kasa uses V1-style method/params wrapper on root path.
        // Tapo uses flat body on /api/v2/common/passthrough.
        let (body, url_path) = match self.cloud_type {
            CloudType::Kasa => {
                let body = json!({
                    "method": "passthrough",
                    "params": {
                        "deviceId": device_id,
                        "requestData": request_data_str,
                    }
                });
                (body, "/")
            }
            CloudType::Tapo => {
                let body = json!({
                    "deviceId": device_id,
                    "requestData": request_data_str,
                });
                (body, "/api/v2/common/passthrough")
            }
        };

        let body_json = serde_json::to_string(&body)?;
        let signing = get_signing_headers(&body_json, url_path, self.cloud_type);

        let url = if url_path == "/" {
            self.host.clone()
        } else {
            format!("{}{}", self.host, url_path)
        };

        if self.verbose {
            eprintln!("[{}] POST {}", self.cloud_type, url);
            eprintln!("Body: {}", body_json);
        }

        let response = self
            .client
            .post(&url)
            .query(&self.query_params)
            .header("Content-Type", "application/json;charset=UTF-8")
            .header("Content-MD5", &signing.content_md5)
            .header("X-Authorization", &signing.x_authorization)
            .body(body_json)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(AppError::BadStatus {
                status: response.status().as_u16(),
            });
        }

        let api_response: ApiResponse = response.json().await?;

        if self.verbose {
            eprintln!(
                "Response: error_code={}, msg={:?}",
                api_response.error_code, api_response.msg
            );
        }

        if api_response.error_code == ERR_TOKEN_EXPIRED {
            return Err(AppError::TokenExpired {
                message: "auth token expired".into(),
                error_code: Some(api_response.error_code),
            });
        }

        if !api_response.successful() {
            return Err(AppError::Api {
                message: api_response
                    .msg
                    .unwrap_or_else(|| format!("Device error code {}", api_response.error_code)),
                error_code: Some(api_response.error_code),
            });
        }

        // Parse the double-encoded responseData
        if let Some(result) = api_response.result {
            if let Some(response_data_str) = result.get("responseData").and_then(|v| v.as_str()) {
                let parsed: serde_json::Value = serde_json::from_str(response_data_str)?;
                if self.verbose {
                    eprintln!(
                        "Passthrough response: {}",
                        serde_json::to_string_pretty(&parsed)?
                    );
                }
                return Ok(Some(parsed));
            }
        }

        Ok(None)
    }
}
