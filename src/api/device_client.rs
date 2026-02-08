use std::collections::HashMap;

use reqwest::Certificate;
use serde_json::json;

use super::cloud_type::CloudType;
use super::errors::*;
use super::response::ApiResponse;
use super::signing::get_signing_headers;
use crate::error::AppError;

const CA_CERT_PEM: &[u8] = include_bytes!("../../certs/tplink-ca-chain.pem");

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
        let cert = Certificate::from_pem(CA_CERT_PEM)?;
        let client = reqwest::Client::builder()
            .add_root_certificate(cert)
            .user_agent("Dalvik/2.1.0 (Linux; U; Android 14; Pixel Build/UP1A)")
            .timeout(std::time::Duration::from_secs(600))
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
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Api {
                message: format!("{}: {}", status, body),
                error_code: None,
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
                message: "Auth token expired".into(),
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
