use std::collections::HashMap;

use reqwest::Certificate;
use serde_json::json;
use uuid::Uuid;

use super::cloud_type::CloudType;
use super::errors::*;
use super::response::ApiResponse;
use super::signing::get_signing_headers;
use crate::error::AppError;

const PATH_ACCOUNT_STATUS: &str = "/api/v2/account/getAccountStatusAndUrl";
const PATH_LOGIN: &str = "/api/v2/account/login";
const PATH_REFRESH_TOKEN: &str = "/api/v2/account/refreshToken";
const PATH_MFA_LOGIN: &str = "/api/v2/account/checkMFACodeAndLogin";

const CA_CERT_PEM: &[u8] = include_bytes!("../../certs/tplink-ca-chain.pem");

pub struct LoginResult {
    pub token: String,
    pub refresh_token: Option<String>,
    pub regional_url: String,
}

pub struct TPLinkApi {
    client: reqwest::Client,
    pub host: String,
    term_id: String,
    cloud_type: CloudType,
    query_params: HashMap<String, String>,
    verbose: bool,
}

fn build_http_client() -> Result<reqwest::Client, AppError> {
    let cert = Certificate::from_pem(CA_CERT_PEM)?;
    Ok(reqwest::Client::builder()
        .add_root_certificate(cert)
        .user_agent("Dalvik/2.1.0 (Linux; U; Android 14; Pixel Build/UP1A)")
        .timeout(std::time::Duration::from_secs(15))
        .build()?)
}

fn build_query_params(cloud_type: CloudType, term_id: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    params.insert("appName".into(), cloud_type.app_type().into());
    params.insert("appVer".into(), cloud_type.app_version().into());
    params.insert("netType".into(), "wifi".into());
    params.insert("termID".into(), term_id.into());
    params.insert("ospf".into(), "Android 14".into());
    params.insert("brand".into(), "TPLINK".into());
    params.insert("locale".into(), "en_US".into());
    params.insert("model".into(), "Pixel".into());
    params.insert("termName".into(), "Pixel".into());
    params.insert("termMeta".into(), "Pixel".into());
    params
}

impl TPLinkApi {
    pub fn new(
        host: Option<String>,
        verbose: bool,
        term_id: Option<String>,
        cloud_type: CloudType,
    ) -> Result<Self, AppError> {
        let term_id = term_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let query_params = build_query_params(cloud_type, &term_id);
        let client = build_http_client()?;

        Ok(Self {
            client,
            host: host.unwrap_or_else(|| cloud_type.host().to_string()),
            term_id,
            cloud_type,
            query_params,
            verbose,
        })
    }

    pub fn term_id(&self) -> &str {
        &self.term_id
    }

    pub fn cloud_type(&self) -> CloudType {
        self.cloud_type
    }

    /// Make a signed V2 API request.
    async fn request_post_v2(
        &self,
        base_url: &str,
        url_path: &str,
        body: &serde_json::Value,
        token: Option<&str>,
    ) -> Result<ApiResponse, AppError> {
        let url = format!("{}{}", base_url, url_path);
        let body_json = serde_json::to_string(body)?;

        let mut params = self.query_params.clone();
        if let Some(token) = token {
            params.insert("token".into(), token.into());
        }

        let signing = get_signing_headers(&body_json, url_path, self.cloud_type);

        if self.verbose {
            eprintln!("[{}] POST {}", self.cloud_type, url);
            eprintln!("Body: {}", body_json);
        }

        let response = self
            .client
            .post(&url)
            .query(&params)
            .header("Content-Type", "application/json;charset=UTF-8")
            .header("Content-MD5", &signing.content_md5)
            .header("X-Authorization", &signing.x_authorization)
            .body(body_json)
            .send()
            .await?;

        if response.status().is_success() {
            let api_response: ApiResponse = response.json().await?;
            if self.verbose {
                eprintln!(
                    "Response: {}",
                    serde_json::to_string_pretty(&json!({
                        "error_code": api_response.error_code,
                        "msg": &api_response.msg,
                    }))?
                );
            }
            Ok(api_response)
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(AppError::Api {
                message: format!("{}: {}", status, body),
                error_code: None,
            })
        }
    }

    /// Make a V1-style request (method/params wrapper) with V2 signing.
    /// Used for Kasa device list and device passthrough operations.
    async fn request_post_v1(
        &self,
        body: &serde_json::Value,
        token: Option<&str>,
    ) -> Result<ApiResponse, AppError> {
        let url_path = "/";
        let body_json = serde_json::to_string(body)?;

        let mut params = self.query_params.clone();
        if let Some(token) = token {
            params.insert("token".into(), token.into());
        }

        let signing = get_signing_headers(&body_json, url_path, self.cloud_type);

        if self.verbose {
            eprintln!("[{}] POST {}/", self.cloud_type, self.host);
            eprintln!("Body: {}", body_json);
        }

        let response = self
            .client
            .post(&self.host)
            .query(&params)
            .header("Content-Type", "application/json;charset=UTF-8")
            .header("Content-MD5", &signing.content_md5)
            .header("X-Authorization", &signing.x_authorization)
            .body(body_json)
            .send()
            .await?;

        if response.status().is_success() {
            let api_response: ApiResponse = response.json().await?;
            if self.verbose {
                eprintln!(
                    "Response: {}",
                    serde_json::to_string_pretty(&json!({
                        "error_code": api_response.error_code,
                        "msg": &api_response.msg,
                    }))?
                );
            }
            Ok(api_response)
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(AppError::Api {
                message: format!("{}: {}", status, body),
                error_code: None,
            })
        }
    }

    /// Discover the regional API server URL for the given account.
    async fn get_regional_url(&self, username: &str) -> Result<String, AppError> {
        let body = json!({
            "appType": self.cloud_type.app_type(),
            "cloudUserName": username,
        });
        let response = self
            .request_post_v2(&self.host, PATH_ACCOUNT_STATUS, &body, None)
            .await?;
        if response.successful() {
            if let Some(result) = &response.result {
                if let Some(url) = result.get("appServerUrl").and_then(|v| v.as_str()) {
                    return Ok(url.to_string());
                }
            }
        }
        Ok(self.host.clone())
    }

    /// Authenticate with the TP-Link Cloud V2 API.
    pub async fn login(&mut self, username: &str, password: &str) -> Result<LoginResult, AppError> {
        if username.is_empty() {
            return Err(AppError::InvalidInput("Username is required".into()));
        }
        if password.is_empty() {
            return Err(AppError::InvalidInput("Password is required".into()));
        }

        // Step 1: Discover regional URL
        let regional_url = self.get_regional_url(username).await?;
        self.host = regional_url.clone();

        // Step 2: Login
        let login_body = json!({
            "appType": self.cloud_type.app_type(),
            "appVersion": self.cloud_type.app_version(),
            "cloudPassword": password,
            "cloudUserName": username,
            "platform": "Android",
            "refreshTokenNeeded": true,
            "supportBindAccount": false,
            "terminalUUID": self.term_id,
            "terminalName": "Pixel",
            "terminalMeta": "Pixel",
        });

        let response = self
            .request_post_v2(&regional_url, PATH_LOGIN, &login_body, None)
            .await?;

        let error_code = response.error_code;
        if error_code == 0 {
            let result = response.result.unwrap_or_default();

            // The V2 API can return error_code 0 at the outer level but
            // include an inner errorCode in the result object (as string or int).
            let inner_error = result
                .get("errorCode")
                .and_then(|v| {
                    v.as_i64()
                        .map(|n| n as i32)
                        .or_else(|| v.as_str().and_then(|s| s.parse::<i32>().ok()))
                })
                .unwrap_or(0);

            if inner_error != 0 {
                let inner_msg = result
                    .get("errorMsg")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Login failed")
                    .to_string();

                if inner_error == ERR_MFA_REQUIRED {
                    return Err(AppError::MfaRequired {
                        mfa_type: result
                            .get("mfaType")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        email: Some(username.to_string()),
                    });
                }

                if inner_error == ERR_WRONG_CREDENTIALS || inner_error == ERR_ACCOUNT_LOCKED {
                    return Err(AppError::Auth {
                        message: inner_msg,
                        error_code: Some(inner_error),
                    });
                }

                return Err(AppError::Api {
                    message: inner_msg,
                    error_code: Some(inner_error),
                });
            }

            return Ok(LoginResult {
                token: result["token"].as_str().unwrap_or_default().to_string(),
                refresh_token: result["refreshToken"].as_str().map(|s| s.to_string()),
                regional_url,
            });
        }

        if error_code == ERR_MFA_REQUIRED {
            let mfa_type = response
                .result
                .as_ref()
                .and_then(|r| r.get("mfaType"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            return Err(AppError::MfaRequired {
                mfa_type,
                email: Some(username.to_string()),
            });
        }

        if error_code == ERR_WRONG_CREDENTIALS || error_code == ERR_ACCOUNT_LOCKED {
            return Err(AppError::Auth {
                message: response
                    .msg
                    .unwrap_or_else(|| "Authentication failed".into()),
                error_code: Some(error_code),
            });
        }

        Err(AppError::Api {
            message: response
                .msg
                .unwrap_or_else(|| format!("Login failed with error code {}", error_code)),
            error_code: Some(error_code),
        })
    }

    /// Complete MFA verification.
    pub async fn verify_mfa(
        &self,
        username: &str,
        password: &str,
        mfa_code: &str,
    ) -> Result<LoginResult, AppError> {
        let body = json!({
            "appType": self.cloud_type.app_type(),
            "cloudPassword": password,
            "cloudUserName": username,
            "code": mfa_code,
            "terminalUUID": self.term_id,
        });

        let response = self
            .request_post_v2(&self.host, PATH_MFA_LOGIN, &body, None)
            .await?;

        if response.successful() {
            let result = response.result.unwrap_or_default();
            return Ok(LoginResult {
                token: result["token"].as_str().unwrap_or_default().to_string(),
                refresh_token: result["refreshToken"].as_str().map(|s| s.to_string()),
                regional_url: self.host.clone(),
            });
        }

        Err(AppError::Auth {
            message: response
                .msg
                .unwrap_or_else(|| "MFA verification failed".into()),
            error_code: Some(response.error_code),
        })
    }

    /// Refresh an expired auth token using a refresh token.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<LoginResult, AppError> {
        let body = json!({
            "appType": self.cloud_type.app_type(),
            "refreshToken": refresh_token,
            "terminalUUID": self.term_id,
        });

        let response = self
            .request_post_v2(&self.host, PATH_REFRESH_TOKEN, &body, None)
            .await?;

        if response.successful() {
            let result = response.result.unwrap_or_default();
            return Ok(LoginResult {
                token: result["token"].as_str().unwrap_or_default().to_string(),
                refresh_token: result["refreshToken"].as_str().map(|s| s.to_string()),
                regional_url: self.host.clone(),
            });
        }

        if response.error_code == ERR_REFRESH_TOKEN_EXPIRED {
            return Err(AppError::TokenExpired {
                message: "Refresh token has expired. Run 'tplc login' to re-authenticate.".into(),
                error_code: Some(response.error_code),
            });
        }

        Err(AppError::Api {
            message: response.msg.unwrap_or_else(|| {
                format!(
                    "Token refresh failed with error code {}",
                    response.error_code
                )
            }),
            error_code: Some(response.error_code),
        })
    }

    /// Get the list of devices registered to the account.
    pub async fn get_device_info_list(
        &self,
        token: &str,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        let body = json!({"method": "getDeviceList"});
        let response = self.request_post_v1(&body, Some(token)).await?;

        if response.successful() {
            if let Some(result) = response.result {
                if let Some(devices) = result.get("deviceList") {
                    if let Some(arr) = devices.as_array() {
                        return Ok(arr.clone());
                    }
                }
            }
            return Ok(vec![]);
        }

        if response.error_code == ERR_TOKEN_EXPIRED {
            return Err(AppError::TokenExpired {
                message: "Auth token expired".into(),
                error_code: Some(response.error_code),
            });
        }

        Ok(vec![])
    }
}
