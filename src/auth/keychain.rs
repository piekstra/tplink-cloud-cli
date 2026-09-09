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

/// The per-field layout older builds wrote, named once: the migration reads
/// exactly these keys and deletes exactly these keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyKey {
    Token,
    RefreshToken,
    Username,
    RegionalUrl,
    TermId,
    TapoToken,
    TapoRefreshToken,
    TapoRegionalUrl,
}

impl LegacyKey {
    const ALL: [LegacyKey; 8] = [
        LegacyKey::Token,
        LegacyKey::RefreshToken,
        LegacyKey::Username,
        LegacyKey::RegionalUrl,
        LegacyKey::TermId,
        LegacyKey::TapoToken,
        LegacyKey::TapoRefreshToken,
        LegacyKey::TapoRegionalUrl,
    ];

    fn name(self) -> &'static str {
        match self {
            LegacyKey::Token => "token",
            LegacyKey::RefreshToken => "refresh_token",
            LegacyKey::Username => "username",
            LegacyKey::RegionalUrl => "regional_url",
            LegacyKey::TermId => "term_id",
            LegacyKey::TapoToken => "tapo_token",
            LegacyKey::TapoRefreshToken => "tapo_refresh_token",
            LegacyKey::TapoRegionalUrl => "tapo_regional_url",
        }
    }
}

/// The legacy items as read, before any interpretation.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LegacyFields {
    pub token: Option<String>,
    pub refresh_token: Option<String>,
    pub username: Option<String>,
    pub regional_url: Option<String>,
    pub term_id: Option<String>,
    pub tapo_token: Option<String>,
    pub tapo_refresh_token: Option<String>,
    pub tapo_regional_url: Option<String>,
}

/// Pure mapping from the legacy items to a session. `None` when there was
/// no session at all (no token); required-but-missing companions fall back
/// to empty strings, as the old reader did, so a partial legacy write still
/// yields a session that `login` can replace.
pub fn migrate(fields: LegacyFields) -> Option<TokenSet> {
    let token = fields.token?;
    Some(TokenSet {
        token,
        refresh_token: fields.refresh_token,
        username: fields.username.unwrap_or_default(),
        regional_url: fields.regional_url.unwrap_or_default(),
        term_id: fields.term_id.unwrap_or_default(),
        tapo_token: fields.tapo_token,
        tapo_refresh_token: fields.tapo_refresh_token,
        tapo_regional_url: fields.tapo_regional_url,
    })
}

fn read_legacy() -> Result<LegacyFields, AppError> {
    let mut f = LegacyFields::default();
    for key in LegacyKey::ALL {
        let v = get_value(key.name())?;
        match key {
            LegacyKey::Token => f.token = v,
            LegacyKey::RefreshToken => f.refresh_token = v,
            LegacyKey::Username => f.username = v,
            LegacyKey::RegionalUrl => f.regional_url = v,
            LegacyKey::TermId => f.term_id = v,
            LegacyKey::TapoToken => f.tapo_token = v,
            LegacyKey::TapoRefreshToken => f.tapo_refresh_token = v,
            LegacyKey::TapoRegionalUrl => f.tapo_regional_url = v,
        }
    }
    Ok(f)
}

fn delete_legacy() -> Result<(), AppError> {
    for key in LegacyKey::ALL {
        delete_value(key.name())?;
    }
    Ok(())
}

pub fn store_tokens(tokens: &TokenSet) -> Result<(), AppError> {
    let blob = serde_json::to_string(tokens).map_err(|e| AppError::Keychain(e.to_string()))?;
    set_value(SESSION_KEY, &blob)
}

/// The stored session. A present-but-unparseable session item is an error,
/// not "never logged in": one blob now carries everything, so a swallowed
/// parse failure would read as an unexplained logout.
pub fn get_tokens() -> Result<Option<TokenSet>, AppError> {
    if let Some(blob) = get_value(SESSION_KEY)? {
        return serde_json::from_str(&blob).map(Some).map_err(|e| {
            AppError::Keychain(format!(
                "stored session is unreadable ({e}); run `tplc logout` then `tplc login`"
            ))
        });
    }
    // One-time migration from the per-field layout. The consolidated item is
    // written first and the legacy items are deleted only after that write
    // succeeded, so a failure midway leaves the old layout intact and a later
    // run retries the migration.
    let Some(migrated) = migrate(read_legacy()?) else {
        return Ok(None);
    };
    store_tokens(&migrated)?;
    delete_legacy()?;
    Ok(Some(migrated))
}

pub fn clear_tokens() -> Result<(), AppError> {
    delete_value(SESSION_KEY)?;
    delete_legacy()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_maps_every_legacy_field() {
        let f = LegacyFields {
            token: Some("kasa-token".into()),
            refresh_token: Some("kasa-refresh".into()),
            username: Some("user@example.com".into()),
            regional_url: Some("https://n-use1-wap.tplinkcloud.com".into()),
            term_id: Some("00000000-0000-4000-8000-000000000000".into()),
            tapo_token: Some("tapo-token".into()),
            tapo_refresh_token: Some("tapo-refresh".into()),
            tapo_regional_url: Some("https://n-use1-wap-gw.tplinkcloud.com".into()),
        };
        let t = migrate(f).expect("a token means a session");
        assert_eq!(t.token, "kasa-token");
        assert_eq!(t.refresh_token.as_deref(), Some("kasa-refresh"));
        assert_eq!(t.username, "user@example.com");
        assert_eq!(t.regional_url, "https://n-use1-wap.tplinkcloud.com");
        assert_eq!(t.term_id, "00000000-0000-4000-8000-000000000000");
        assert_eq!(t.tapo_token.as_deref(), Some("tapo-token"));
        assert_eq!(t.tapo_refresh_token.as_deref(), Some("tapo-refresh"));
        assert_eq!(
            t.tapo_regional_url.as_deref(),
            Some("https://n-use1-wap-gw.tplinkcloud.com")
        );
    }

    #[test]
    fn migrate_tolerates_absent_optionals_and_needs_a_token() {
        let f = LegacyFields {
            token: Some("kasa-token".into()),
            ..Default::default()
        };
        let t = migrate(f).unwrap();
        assert!(t.refresh_token.is_none() && t.tapo_token.is_none());
        assert_eq!(t.username, "");
        assert!(migrate(LegacyFields::default()).is_none());
    }

    #[test]
    fn legacy_key_names_are_the_ones_older_builds_wrote() {
        let names: Vec<&str> = LegacyKey::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(
            names,
            [
                "token",
                "refresh_token",
                "username",
                "regional_url",
                "term_id",
                "tapo_token",
                "tapo_refresh_token",
                "tapo_regional_url"
            ]
        );
    }

    #[test]
    fn session_blob_round_trips() {
        let t = TokenSet {
            token: "k".into(),
            refresh_token: None,
            username: "u".into(),
            regional_url: "r".into(),
            term_id: "t".into(),
            tapo_token: Some("tt".into()),
            tapo_refresh_token: None,
            tapo_regional_url: None,
        };
        let blob = serde_json::to_string(&t).unwrap();
        let back: TokenSet = serde_json::from_str(&blob).unwrap();
        assert_eq!(back.token, "k");
        assert_eq!(back.tapo_token.as_deref(), Some("tt"));
        assert!(serde_json::from_str::<TokenSet>("{not json").is_err());
    }
}
