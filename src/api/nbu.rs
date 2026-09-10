//! The Tapo "NBU app-server" cloud — where the Tapo app keeps homes
//! (families), rooms, and which room each device (thing) is in. Decoded
//! from the Tapo Android app (see `docs/api.md`, "Rooms").
//!
//! It is a different host from the v2 account cloud, resolved per account
//! via `getAppServiceUrl` (see [`super::client::TPLinkApi::get_app_service_url`]),
//! and it is authenticated with the Tapo v2 login token as `Authorization:
//! ut|<token>` plus a fixed set of app-identity headers — no HMAC signing.
//! Responses are bare JSON (no `error_code` envelope); errors are HTTP
//! statuses with a `{"code", "message"}` body.

use crate::models::device_info::decode_encoded_name;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, USER_AGENT};
use reqwest::{Certificate, Method};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::AppError;

/// The service id whose URL `getAppServiceUrl` resolves to this cloud's host.
pub const APP_SERVER_SERVICE_ID: &str = "nbu.iot-app-server.app-v2";
/// The Tapo app version these headers were decoded from.
pub const APP_VERSION: &str = "3.20.753";
const APP_NAME: &str = "TP-Link_Tapo_Android";
const PAGE_SIZE: u32 = 20;
/// Pagination safety stop (a home has tens of devices, not thousands).
const MAX_PAGES: u32 = 50;

const CA_CERT_PEM: &[u8] = include_bytes!("../../certs/tplink-ca-chain.pem");

/// A home, with its rooms embedded (`GET /v1/families`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Family {
    pub id: String,
    pub name: String,
    #[serde(default, rename = "default")]
    pub is_default: bool,
    #[serde(default)]
    pub rooms: Vec<Room>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

/// One device as the Tapo cloud lists it (`GET /v2/things`). `thing_name`
/// is the device id the account cloud (and Google Home) know it by; room
/// membership is `family_id`/`room_id`. Only the fields the CLI reads.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thing {
    pub thing_name: String,
    #[serde(default)]
    pub family_id: Option<String>,
    #[serde(default)]
    pub room_id: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub device_model: Option<String>,
    #[serde(default)]
    pub device_type: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub status: Option<i64>,
    #[serde(default)]
    pub mac: Option<String>,
}

impl Thing {
    /// The id Google Home shows for this device (`partner_device_id`), so
    /// `device-rooms/v1` rows join: Tapo's own integration reports the MAC
    /// without separators, while Kasa devices shared into the Tapo app keep
    /// their Kasa device id (the thing name).
    pub fn google_id(&self) -> String {
        match (&self.mac, self.is_native_tapo()) {
            (Some(mac), true) if !mac.trim().is_empty() => mac
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_uppercase(),
            _ => self.thing_name.clone(),
        }
    }

    /// Whether this is one of Tapo's own devices rather than a Kasa device
    /// shared into the Tapo app (`includeKasaShareDevices`).
    pub fn is_native_tapo(&self) -> bool {
        self.device_type
            .as_deref()
            .is_some_and(|t| t.to_uppercase().starts_with("SMART.TAPO"))
    }

    /// The user-facing name, decoded the way the account cloud's alias is
    /// (`models::device_info::decode_encoded_name`).
    pub fn display_name(&self) -> Option<String> {
        let raw = self.nickname.as_deref()?.trim();
        if raw.is_empty() {
            return None;
        }
        Some(decode_encoded_name(raw))
    }
}

/// A fresh room id the way the app makes one: 8 chars of `[A-Za-z0-9]`.
pub fn new_room_id() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    uuid::Uuid::new_v4()
        .as_bytes()
        .iter()
        .take(8)
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

pub struct NbuClient {
    client: reqwest::Client,
    base: String,
    token: String,
    verbose: bool,
}

impl NbuClient {
    pub fn new(base: &str, token: &str, term_id: &str, verbose: bool) -> Result<Self, AppError> {
        let cert = Certificate::from_pem(CA_CERT_PEM)?;
        let mut headers = HeaderMap::new();
        let put = |headers: &mut HeaderMap, k: &'static str, v: String| {
            if let Ok(hv) = HeaderValue::from_str(&v) {
                headers.insert(k, hv);
            }
        };
        put(&mut headers, "app-cid", format!("app:{APP_NAME}:{term_id}"));
        put(&mut headers, "x-app-name", APP_NAME.into());
        put(&mut headers, "x-app-version", APP_VERSION.into());
        put(&mut headers, "x-term-id", term_id.into());
        put(&mut headers, "x-ospf", "Android 14".into());
        put(&mut headers, "x-net-type", "wifi".into());
        put(&mut headers, "x-strict", "0".into());
        put(&mut headers, "x-locale", "en_US".into());
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("TP-Link_Tapo_Android/3.20.753(Pixel/;Android 14)"),
        );
        let client = reqwest::Client::builder()
            .add_root_certificate(cert)
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(NbuClient {
            client,
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
            verbose,
        })
    }

    /// The same connection (the HTTP client is shared behind an `Arc`)
    /// presenting a different token — what a retry after a refresh uses.
    pub fn with_token(&self, token: &str) -> NbuClient {
        NbuClient {
            client: self.client.clone(),
            base: self.base.clone(),
            token: token.to_string(),
            verbose: self.verbose,
        }
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Value, AppError> {
        let url = format!("{}{}", self.base, path);
        if self.verbose {
            eprintln!("[nbu] {method} {url}");
            if let Some(b) = body {
                eprintln!("Body: {b}");
            }
        }
        let mut req = self
            .client
            .request(method, &url)
            .header(AUTHORIZATION, format!("ut|{}", self.token))
            .query(query);
        if let Some(b) = body {
            req = req
                .header("Content-Type", "application/json;charset=UTF-8")
                .body(serde_json::to_string(b)?);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if self.verbose {
            eprintln!("HTTP {} ({} bytes)", status.as_u16(), text.len());
        }
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(AppError::TokenExpired {
                message: format!("Tapo cloud rejected the token (HTTP {})", status.as_u16()),
                error_code: None,
            });
        }
        let parsed: Option<Value> = if text.trim().is_empty() {
            None
        } else {
            serde_json::from_str(&text).ok()
        };
        if !status.is_success() {
            let (msg, code) = match &parsed {
                Some(v) => (
                    v.get("message")
                        .or(v.get("msg"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    v.get("code")
                        .or(v.get("errorCode"))
                        .and_then(Value::as_i64)
                        .map(|c| c as i32),
                ),
                None => (text.chars().take(300).collect(), None),
            };
            return Err(AppError::Api {
                message: format!("HTTP {} from the Tapo cloud: {msg}", status.as_u16()),
                error_code: code.or(Some(status.as_u16() as i32)),
            });
        }
        Ok(parsed.unwrap_or(Value::Null))
    }

    /// Walk a `PageListResult` (`{page, pageSize, total, data}`) to the end.
    async fn paged(&self, path: &str, extra: &[(&str, String)]) -> Result<Vec<Value>, AppError> {
        let mut out = Vec::new();
        for page in 0..MAX_PAGES {
            let mut query: Vec<(&str, String)> = vec![
                ("page", page.to_string()),
                ("pageSize", PAGE_SIZE.to_string()),
            ];
            query.extend(extra.iter().cloned());
            let v = self.call(Method::GET, path, &query, None).await?;
            let data = v
                .get("data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let total = v.get("total").and_then(Value::as_u64).unwrap_or(0) as usize;
            let got = data.len();
            out.extend(data);
            if got == 0 || out.len() >= total {
                break;
            }
        }
        Ok(out)
    }

    /// Every home on the account, rooms embedded.
    pub async fn families(&self) -> Result<Vec<Family>, AppError> {
        let raw = self.paged("/v1/families", &[]).await?;
        Ok(raw
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect())
    }

    /// Every device the Tapo cloud lists, with its room.
    pub async fn things(&self) -> Result<Vec<Thing>, AppError> {
        let extra = [
            ("includePcDevice", "true".to_string()),
            ("includeKasaShareDevices", "true".to_string()),
            ("includeMatterDevice", "true".to_string()),
            ("includeExternalVendorDeviceInfo", "true".to_string()),
        ];
        let raw = self.paged("/v2/things", &extra).await?;
        Ok(raw
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect())
    }

    /// Put devices in a room (`POST /v1/families/thing-settings`); the
    /// response is empty, so the caller reads `things` back to verify.
    pub async fn move_things(
        &self,
        family_id: &str,
        room_id: &str,
        thing_names: &[String],
    ) -> Result<(), AppError> {
        let body = json!({"familyId": family_id, "roomId": room_id, "thingNames": thing_names});
        self.call(
            Method::POST,
            "/v1/families/thing-settings",
            &[],
            Some(&body),
        )
        .await?;
        Ok(())
    }

    /// Create (new id) or rename (existing id) a room — the endpoint is an
    /// upsert on `id`.
    pub async fn upsert_room(&self, family_id: &str, id: &str, name: &str) -> Result<(), AppError> {
        let body = json!({"id": id, "name": name});
        self.call(
            Method::PUT,
            &format!("/v1/families/{family_id}/rooms"),
            &[],
            Some(&body),
        )
        .await?;
        Ok(())
    }

    pub async fn delete_room(&self, family_id: &str, room_id: &str) -> Result<(), AppError> {
        self.call(
            Method::DELETE,
            &format!("/v1/families/{family_id}/rooms/{room_id}"),
            &[],
            None,
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_ids_are_eight_alphanumerics() {
        let id = new_room_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(id, new_room_id());
    }

    #[test]
    fn family_and_thing_shapes_deserialize() {
        let f: Family = serde_json::from_value(json!({
            "id": "FAM00001", "name": "Home", "default": true,
            "rooms": [{"id": "ROOM0001", "name": "Office", "avatarUrl": ""}]
        }))
        .unwrap();
        assert!(f.is_default);
        assert_eq!(f.rooms[0].name, "Office");
        let t: Thing = serde_json::from_value(json!({
            "thingName": "0000000000000000000000000000000000000001",
            "familyId": "FAM00001", "roomId": "ROOM0001", "nickname": "T2ZmaWNlIExhbXA="
        }))
        .unwrap();
        assert_eq!(t.display_name().as_deref(), Some("Office Lamp"));
        let bare: Thing = serde_json::from_value(json!({"thingName": "x"})).unwrap();
        assert!(bare.room_id.is_none() && bare.display_name().is_none());
    }
}

#[cfg(test)]
mod google_id_tests {
    use super::*;

    fn thing(name: &str, kind: &str, mac: Option<&str>) -> Thing {
        Thing {
            thing_name: name.into(),
            family_id: None,
            room_id: None,
            nickname: None,
            device_model: None,
            device_type: Some(kind.into()),
            category: None,
            status: None,
            mac: mac.map(str::to_string),
        }
    }

    #[test]
    fn tapo_devices_join_on_mac_and_kasa_devices_on_their_id() {
        assert_eq!(
            thing("abc123", "SMART.TAPOLOCK", Some("10:5a:95:2f:ad:17")).google_id(),
            "105A952FAD17"
        );
        assert_eq!(
            thing("8006ABCD", "SMART.KASAPLUG", Some("00:11:22:33:44:55")).google_id(),
            "8006ABCD"
        );
        assert_eq!(
            thing("abc123", "SMART.TAPOPLUG", None).google_id(),
            "abc123"
        );
    }
}
