use keyring::Entry;

use crate::auth::token::TokenSet;
use crate::error::AppError;

const SERVICE: &str = "tplc";

fn entry(key: &str) -> Result<Entry, AppError> {
    Entry::new(SERVICE, key).map_err(|e| AppError::Keychain(e.to_string()))
}

fn get_value(key: &str) -> Result<Option<String>, AppError> {
    let entry = entry(key)?;
    match entry.get_password() {
        Ok(val) => Ok(Some(val)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Keychain(e.to_string())),
    }
}

fn set_value(key: &str, value: &str) -> Result<(), AppError> {
    let entry = entry(key)?;
    entry
        .set_password(value)
        .map_err(|e| AppError::Keychain(e.to_string()))
}

fn delete_value(key: &str) -> Result<(), AppError> {
    let entry = entry(key)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Keychain(e.to_string())),
    }
}

/// One keychain item holds the whole session. Every item read is a macOS
/// permission prompt for a freshly built binary, and the old layout kept
/// eight of them; a single JSON blob is one prompt.
const SESSION_KEY: &str = "session";
const LEGACY_KEYS: &[&str] = &[
    "token",
    "refresh_token",
    "username",
    "regional_url",
    "term_id",
    "tapo_token",
    "tapo_refresh_token",
    "tapo_regional_url",
];

pub fn store_tokens(tokens: &TokenSet) -> Result<(), AppError> {
    let blob = serde_json::to_string(tokens).map_err(|e| AppError::Keychain(e.to_string()))?;
    set_value(SESSION_KEY, &blob)
}

pub fn get_tokens() -> Result<Option<TokenSet>, AppError> {
    if let Some(blob) = get_value(SESSION_KEY)? {
        return Ok(serde_json::from_str(&blob).ok());
    }
    // One-time migration from the per-field layout: read it, write the
    // single item, and drop the old items so they never prompt again.
    let token = match get_value("token")? {
        Some(t) => t,
        None => return Ok(None),
    };
    let migrated = TokenSet {
        token,
        refresh_token: get_value("refresh_token")?,
        username: get_value("username")?.unwrap_or_default(),
        regional_url: get_value("regional_url")?.unwrap_or_default(),
        term_id: get_value("term_id")?.unwrap_or_default(),
        tapo_token: get_value("tapo_token")?,
        tapo_refresh_token: get_value("tapo_refresh_token")?,
        tapo_regional_url: get_value("tapo_regional_url")?,
    };
    store_tokens(&migrated)?;
    for k in LEGACY_KEYS {
        delete_value(k)?;
    }
    Ok(Some(migrated))
}

pub fn clear_tokens() -> Result<(), AppError> {
    delete_value(SESSION_KEY)?;
    for k in LEGACY_KEYS {
        delete_value(k)?;
    }
    Ok(())
}

const PENDING_KEY: &str = "mfa_pending";

/// An in-flight login that stopped for an MFA code: the terminal id the code
/// was requested for must be reused, or the code is presented against a
/// session that never asked for one.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct PendingLogin {
    pub username: String,
    pub term_id: String,
    /// "kasa" or "tapo": the cloud waiting for a code.
    pub cloud: String,
    /// Kasa tokens already obtained when Tapo is the one waiting.
    pub kasa_token: Option<String>,
    pub kasa_refresh_token: Option<String>,
    pub kasa_regional_url: Option<String>,
}

pub fn store_pending(p: &PendingLogin) -> Result<(), AppError> {
    let blob = serde_json::to_string(p).map_err(|e| AppError::Keychain(e.to_string()))?;
    Entry::new(SERVICE, PENDING_KEY)
        .and_then(|e| e.set_password(&blob))
        .map_err(|e| AppError::Keychain(e.to_string()))
}

pub fn get_pending() -> Result<Option<PendingLogin>, AppError> {
    match Entry::new(SERVICE, PENDING_KEY).and_then(|e| e.get_password()) {
        Ok(blob) => Ok(serde_json::from_str(&blob).ok()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Keychain(e.to_string())),
    }
}

pub fn clear_pending() -> Result<(), AppError> {
    match Entry::new(SERVICE, PENDING_KEY).and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Keychain(e.to_string())),
    }
}
