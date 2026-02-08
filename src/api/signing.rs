use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use sha1::Sha1;
use uuid::Uuid;

/// App-level keys from Kasa Android APK (identify the app, not the user).
const ACCESS_KEY: &str = "e37525375f8845999bcc56d5e6faa76d";
const SECRET_KEY: &str = "314bc6700b3140ca80bc655e527cb062";

/// The Kasa app uses a hardcoded timestamp for signing.
const SIGNING_TIMESTAMP: &str = "9999999999";

type HmacSha1 = Hmac<Sha1>;

pub struct SigningHeaders {
    pub content_md5: String,
    pub x_authorization: String,
}

/// Compute Base64-encoded MD5 hash of the request body.
pub fn compute_content_md5(body: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(body.as_bytes());
    STANDARD.encode(hasher.finalize())
}

/// Compute HMAC-SHA1 signature for a V2 API request.
///
/// Returns (content_md5, x_authorization_header).
pub fn compute_signature(body_json: &str, url_path: &str) -> (String, String) {
    let content_md5 = compute_content_md5(body_json);
    let nonce = Uuid::new_v4().to_string();

    let sig_string = format!(
        "{}\n{}\n{}\n{}",
        content_md5, SIGNING_TIMESTAMP, nonce, url_path
    );

    let mut mac =
        HmacSha1::new_from_slice(SECRET_KEY.as_bytes()).expect("HMAC accepts any key length");
    mac.update(sig_string.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let authorization = format!(
        "Timestamp={}, Nonce={}, AccessKey={}, Signature={}",
        SIGNING_TIMESTAMP, nonce, ACCESS_KEY, signature
    );

    (content_md5, authorization)
}

/// Get the headers required for a signed V2 API request.
pub fn get_signing_headers(body_json: &str, url_path: &str) -> SigningHeaders {
    let (content_md5, x_authorization) = compute_signature(body_json, url_path);
    SigningHeaders {
        content_md5,
        x_authorization,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_content_md5() {
        let body = r#"{"appType":"Kasa_Android_Mix","cloudUserName":"test@example.com"}"#;
        let md5 = compute_content_md5(body);
        // MD5 should be a base64-encoded string
        assert!(!md5.is_empty());
        // Base64 should decode back to 16 bytes (MD5 digest size)
        let decoded = STANDARD.decode(&md5).unwrap();
        assert_eq!(decoded.len(), 16);
    }

    #[test]
    fn test_compute_signature_produces_valid_format() {
        let body = r#"{"test":"data"}"#;
        let url_path = "/api/v2/account/login";

        let (content_md5, authorization) = compute_signature(body, url_path);

        assert!(!content_md5.is_empty());
        assert!(authorization.starts_with("Timestamp=9999999999, Nonce="));
        assert!(authorization.contains("AccessKey=e37525375f8845999bcc56d5e6faa76d"));
        assert!(authorization.contains("Signature="));
    }

    #[test]
    fn test_get_signing_headers() {
        let body = r#"{"test":"data"}"#;
        let url_path = "/";

        let headers = get_signing_headers(body, url_path);

        assert!(!headers.content_md5.is_empty());
        assert!(headers.x_authorization.contains("Timestamp="));
        assert!(headers.x_authorization.contains("Nonce="));
        assert!(headers.x_authorization.contains("AccessKey="));
        assert!(headers.x_authorization.contains("Signature="));
    }

    #[test]
    fn test_different_bodies_produce_different_md5() {
        let md5_a = compute_content_md5(r#"{"a":"1"}"#);
        let md5_b = compute_content_md5(r#"{"b":"2"}"#);
        assert_ne!(md5_a, md5_b);
    }

    #[test]
    fn test_same_body_produces_same_md5() {
        let body = r#"{"same":"body"}"#;
        let md5_a = compute_content_md5(body);
        let md5_b = compute_content_md5(body);
        assert_eq!(md5_a, md5_b);
    }
}
