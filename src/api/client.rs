use std::collections::HashMap;

use pk_cli_core::transport::{require_secure_url, same_host_redirects};
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
const PATH_APP_SERVICE_URL: &str = "/api/v2/common/getAppServiceUrl";

const CA_CERT_PEM: &[u8] = include_bytes!("../../certs/tplink-ca-chain.pem");

/// Body keys whose values never reach stderr, even under `-v`: the account
/// password (login, MFA), tokens (refresh), and the emailed MFA code.
const REDACTED_KEYS: &[&str] = &[
    "cloudPassword",
    "password",
    "token",
    "refreshToken",
    "accessToken",
    "code",
];

/// A request body as it may be logged: every secret-bearing value replaced
/// by `<redacted>`, at any depth. The only path a body takes to stderr.
pub fn redacted(body: &serde_json::Value) -> String {
    fn scrub(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(map) => {
                for (k, x) in map.iter_mut() {
                    if REDACTED_KEYS.contains(&k.as_str()) {
                        *x = serde_json::Value::String("<redacted>".into());
                    } else {
                        scrub(x);
                    }
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(scrub),
            _ => {}
        }
    }
    let mut copy = body.clone();
    scrub(&mut copy);
    copy.to_string()
}

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

/// The domains a URL from TP-Link's answers may take a credential to (docs/api.md, "Clouds"). The
/// login step's answer names the host the password then goes to, so a URL is checked here and not
/// taken on trust.
const PROVIDER_DOMAINS: [&str; 3] = ["tplinkcloud.com", "tplinknbu.com", "tplinkra.com"];

/// Refuse a URL a credential is about to follow, unless it is `https` on one of TP-Link's domains.
/// Such a URL came from TP-Link's answer or from a session made of those answers, so a refusal is
/// an upstream failure, and its text does not repeat what the answer said, since that can reach a
/// model. A debug build, which is what the tests run, also takes this machine, for their stubs.
pub(crate) fn ensure_provider_url(url: &str) -> Result<(), AppError> {
    let host = require_secure_url(url)
        .ok()
        .and_then(|()| reqwest::Url::parse(url).ok())
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase));
    let allowed = host.is_some_and(|host| {
        PROVIDER_DOMAINS
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
            || (cfg!(debug_assertions) && (host == "127.0.0.1" || host == "localhost"))
    });
    if allowed {
        return Ok(());
    }
    Err(AppError::Api {
        message: "TP-Link named an address that is not on a TP-Link domain, so no credential \
                  was sent to it"
            .into(),
        error_code: None,
    })
}

/// A client that keeps a redirect on its host. A signed POST carries a password or a refresh token
/// in its body, which a 307 or 308 re-sends, and a token in its query, so none may leave the host.
pub(crate) fn client_builder() -> Result<reqwest::ClientBuilder, AppError> {
    Ok(reqwest::Client::builder()
        .add_root_certificate(Certificate::from_pem(CA_CERT_PEM)?)
        .redirect(same_host_redirects()))
}

fn build_http_client() -> Result<reqwest::Client, AppError> {
    Ok(client_builder()?
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
        if let Some(host) = &host {
            ensure_provider_url(host)?;
        }
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
            eprintln!("Body: {}", redacted(body));
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
            Err(AppError::BadStatus {
                status: response.status().as_u16(),
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
            eprintln!("Body: {}", redacted(body));
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
            Err(AppError::BadStatus {
                status: response.status().as_u16(),
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
                    ensure_provider_url(url)?;
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
            // An empty token stored would read as "not logged in" for no reason anyone can see.
            let token = result["token"]
                .as_str()
                .filter(|t| !t.is_empty())
                .ok_or_else(|| AppError::Api {
                    message: "the token refresh answered with no token".into(),
                    error_code: None,
                })?;
            return Ok(LoginResult {
                token: token.to_string(),
                refresh_token: result["refreshToken"].as_str().map(|s| s.to_string()),
                regional_url: self.host.clone(),
            });
        }

        if response.error_code == ERR_REFRESH_TOKEN_EXPIRED {
            return Err(AppError::TokenExpired {
                message: "refresh token expired; run `tplc auth login`".into(),
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

    /// Resolve the host of one of the account's NBU services (the Tapo
    /// app-server that keeps rooms, for instance) — `getAppServiceUrl`,
    /// signed like every v2 call, token in the query. The app caches the
    /// answer for 24h; so does `tplc` (in the session).
    pub async fn get_app_service_url(
        &self,
        token: &str,
        service_id: &str,
    ) -> Result<String, AppError> {
        let body = json!({ "serviceIds": [service_id] });
        let response = self
            .request_post_v2(&self.host, PATH_APP_SERVICE_URL, &body, Some(token))
            .await?;
        if response.error_code == ERR_TOKEN_EXPIRED {
            return Err(AppError::TokenExpired {
                message: "auth token expired".into(),
                error_code: Some(response.error_code),
            });
        }
        if !response.successful() {
            return Err(AppError::Api {
                message: response
                    .msg
                    .unwrap_or_else(|| "getAppServiceUrl failed".into()),
                error_code: Some(response.error_code),
            });
        }
        let url = response
            .result
            .as_ref()
            .and_then(|r| r.get("serviceUrls"))
            .and_then(|u| u.get(service_id))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .ok_or_else(|| AppError::Api {
                message: format!("getAppServiceUrl returned no URL for {service_id}"),
                error_code: None,
            })?;
        ensure_provider_url(&url)?;
        Ok(url)
    }

    /// Call any cloud method by name (`{"method": …, "params": …}` on the
    /// account endpoint) and return the raw response. The passthrough for
    /// methods the CLI doesn't model yet, such as the device-group (room) API.
    pub async fn call_method(
        &self,
        token: &str,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<ApiResponse, AppError> {
        let body = match params {
            Some(p) => json!({"method": method, "params": p}),
            None => json!({"method": method}),
        };
        self.request_post_v1(&body, Some(token)).await
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
                message: "auth token expired".into(),
                error_code: Some(response.error_code),
            });
        }

        // Any other failure is reported, not read as an empty account.
        Err(AppError::Api {
            message: response
                .msg
                .unwrap_or_else(|| "getDeviceList failed".into()),
            error_code: Some(response.error_code),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pk_cli_core::CliError;

    /// A server on 127.0.0.1 answering each request with `answer`, and what arrived there.
    fn stub(answer: String) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                log.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                let _ = stream.write_all(answer.as_bytes());
            }
        });
        (url, seen)
    }

    fn json_answer(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn a_credential_goes_only_to_https_on_a_tplink_domain() {
        for good in [
            "https://n-wap.tplinkcloud.com",
            "https://n-use1-wap.tplinkcloud.com/x",
            "https://use1-app-server.iot.i.tplinknbu.com",
            "https://api.tplinkra.com",
            "HTTPS://N-WAP.TPLINKCLOUD.COM",
        ] {
            assert!(ensure_provider_url(good).is_ok(), "{good}");
        }
        for bad in [
            "http://n-wap.tplinkcloud.com",
            "https://elsewhere.example",
            "https://n-wap.tplinkcloud.com.elsewhere.example",
            "https://elsewheretplinkcloud.com",
            "https://n-wap.tplinkcloud.com@elsewhere.example",
            "n-wap.tplinkcloud.com",
            "",
        ] {
            let err = ensure_provider_url(bad).unwrap_err();
            let text = CliError::from(err).to_string();
            assert!(text.contains("no credential"), "{bad}: {text}");
            assert!(!text.contains("elsewhere"), "{bad}: {text}");
        }
    }

    /// The step before a login answers with the host the password goes to.
    #[test]
    fn a_login_answer_naming_another_host_gets_no_password() {
        let (cloud, seen) = stub(json_answer(
            r#"{"error_code":0,"result":{"appServerUrl":"https://elsewhere.example"}}"#,
        ));
        let mut api = TPLinkApi::new(Some(cloud), false, None, CloudType::Kasa).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let Err(err) = runtime.block_on(api.login("someone@example.com", "dummy-password")) else {
            panic!("the login went ahead");
        };
        assert!(CliError::from(err).to_string().contains("no credential"));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(!seen[0].contains("dummy-password"));
    }

    #[test]
    fn a_stored_session_on_another_host_gets_no_token() {
        let stored = Some("https://elsewhere.example".to_string());
        assert!(TPLinkApi::new(stored, false, None, CloudType::Kasa).is_err());
    }

    /// A 307 re-sends the body, which holds the password or the refresh token.
    #[test]
    fn a_body_does_not_follow_a_redirect_to_another_host() {
        let (elsewhere, arrived) = stub(json_answer("{}"));
        let (first, _) = stub(format!(
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: {elsewhere}/x\r\nContent-Length: 0\r\n\r\n"
        ));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let sent = runtime.block_on(async {
            build_http_client()
                .unwrap()
                .post(&first)
                .json(&json!({"cloudPassword": "dummy-password"}))
                .send()
                .await
        });
        let err = AppError::from(sent.unwrap_err());
        assert!(arrived.lock().unwrap().is_empty());
        assert!(matches!(err, AppError::Api { .. }), "{err}");
        let text = CliError::from(err);
        assert_eq!(text.exit_code(), 5);
        assert!(text.to_string().contains("another host"), "{text}");
    }

    #[test]
    fn verbose_rendering_of_a_login_body_carries_no_secret() {
        let body = json!({
            "appType": "Kasa_Android_Mix",
            "cloudUserName": "user@example.com",
            "cloudPassword": "hunter2-secret",
            "terminalUUID": "00000000-0000-4000-8000-000000000000",
            "nested": {"code": "123456", "refreshToken": "rt-secret", "keep": "me"}
        });
        let out = redacted(&body);
        assert!(!out.contains("hunter2-secret"), "{out}");
        assert!(!out.contains("123456"), "{out}");
        assert!(!out.contains("rt-secret"), "{out}");
        assert!(out.contains("user@example.com") && out.contains("Kasa_Android_Mix"));
        assert!(out.contains(r#""keep":"me""#), "{out}");
        assert_eq!(out.matches("<redacted>").count(), 3);
        // A refresh body hides its token; a passthrough body is untouched.
        let refresh = json!({"appType": "x", "refreshToken": "rt", "terminalUUID": "t"});
        assert!(!redacted(&refresh).contains(r#""rt""#));
        let passthrough = json!({"method": "getDeviceList"});
        assert_eq!(redacted(&passthrough), passthrough.to_string());
    }
}
