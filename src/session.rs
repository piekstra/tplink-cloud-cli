//! The stored session: one keychain item under `piekstra.tplc` holding the
//! Kasa and Tapo tokens, plus a second item for a login parked mid-MFA.
//!
//! Keychain layout (service `piekstra.tplc`):
//!   `session`     → JSON [`TokenSet`]
//!   `mfa_pending` → JSON [`PendingLogin`] (only while a login is parked)
//!
//! Older builds used the unprefixed service `tplc`, first as eight
//! per-field items and later as the same two JSON items. Both layouts are
//! migrated on first use — the two-item one by
//! `CredentialStore::migrate_from` (read old → write new → delete old,
//! converging on repeated runs), the per-field one here, in the same order —
//! so a failure midway leaves the old layout intact and the next run retries.
//!
//! No password is ever stored. `auth login` exchanges it for tokens and
//! forgets it.

use pk_cli_core::CliError;
use pk_cli_secrets::CredentialStore;
use serde::{Deserialize, Serialize};

use crate::api::client::TPLinkApi;
use crate::api::cloud_type::CloudType;
use crate::error::AppError;

pub const BIN: &str = "tplc";
/// The pre-spec keychain service name (read for migration only).
const LEGACY_SERVICE: &str = "tplc";
const SESSION_KEY: &str = "session";
const PENDING_KEY: &str = "mfa_pending";

/// Everything a logged-in invocation needs. Kasa is required; Tapo is
/// best-effort (a TP-Link account may have no Tapo devices).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub token: String,
    pub refresh_token: Option<String>,
    pub username: String,
    pub regional_url: String,
    pub term_id: String,
    pub tapo_token: Option<String>,
    pub tapo_refresh_token: Option<String>,
    pub tapo_regional_url: Option<String>,
    /// The Tapo NBU app-server host (rooms live there), resolved per
    /// account via `getAppServiceUrl` and cached for a day like the app does.
    /// Additive: absent in blobs older builds wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tapo_app_server_url: Option<String>,
    /// Unix seconds after which `tapo_app_server_url` is re-resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tapo_app_server_expires_at: Option<u64>,
}

impl TokenSet {
    pub fn has_tapo(&self) -> bool {
        self.tapo_token.as_ref().is_some_and(|t| !t.is_empty())
    }

    /// Token and regional URL for one cloud; `NotAuthenticated` when the
    /// session never reached that cloud.
    pub fn cloud_access(&self, cloud: CloudType) -> Result<(String, String), AppError> {
        match cloud {
            CloudType::Kasa => Ok((self.token.clone(), self.regional_url.clone())),
            CloudType::Tapo => {
                let token = self
                    .tapo_token
                    .clone()
                    .filter(|t| !t.is_empty())
                    .ok_or(AppError::NotAuthenticated)?;
                let url = self
                    .tapo_regional_url
                    .clone()
                    .ok_or(AppError::NotAuthenticated)?;
                Ok((token, url))
            }
        }
    }
}

/// An in-flight login that stopped for an MFA code: the terminal id the code
/// was requested for must be reused, or the code is presented against a
/// session that never asked for one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingLogin {
    pub username: String,
    pub term_id: String,
    /// The cloud waiting for a code.
    pub cloud: CloudType,
    /// Kasa tokens already obtained when Tapo is the one waiting.
    pub kasa_token: Option<String>,
    pub kasa_refresh_token: Option<String>,
    pub kasa_regional_url: Option<String>,
}

/// The per-field layout the oldest builds wrote, named once: the migration
/// reads exactly these keys and deletes exactly these keys.
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

/// The legacy per-field items as read, before any interpretation.
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
        tapo_app_server_url: None,
        tapo_app_server_expires_at: None,
    })
}

/// The keychain-backed session store: the family service plus the legacy
/// one it migrates from.
pub struct Sessions {
    store: CredentialStore,
    legacy: CredentialStore,
}

impl Default for Sessions {
    fn default() -> Self {
        Self::new()
    }
}

impl Sessions {
    pub fn new() -> Self {
        Sessions {
            store: CredentialStore::for_binary(BIN),
            legacy: CredentialStore::new(LEGACY_SERVICE),
        }
    }

    /// The keychain service name sessions live under (`piekstra.tplc`).
    pub fn service(&self) -> &str {
        self.store.service()
    }

    /// The two-item legacy layout, moved to the family service. Converges:
    /// once nothing is left under `tplc`, this is two no-entry lookups.
    fn migrate_service(&self) -> Result<(), CliError> {
        self.store.migrate_from(
            &self.legacy,
            &[(SESSION_KEY, SESSION_KEY), (PENDING_KEY, PENDING_KEY)],
        )?;
        Ok(())
    }

    pub fn store(&self, tokens: &TokenSet) -> Result<(), CliError> {
        self.store.set_json(SESSION_KEY, tokens)
    }

    /// The stored session, migrating any legacy layout on the way. A
    /// present-but-unparseable item is an error (from `get_json`), not
    /// "never logged in": one blob carries everything, so a swallowed parse
    /// failure would read as an unexplained logout.
    pub fn load(&self) -> Result<Option<TokenSet>, CliError> {
        self.migrate_service()?;
        if let Some(tokens) = self.store.get_json::<TokenSet>(SESSION_KEY)? {
            return Ok(Some(tokens));
        }
        // The oldest layout: eight per-field items under `tplc`.
        let Some(tokens) = migrate(self.read_legacy_fields()?) else {
            return Ok(None);
        };
        self.store(&tokens)?;
        self.delete_legacy_fields()?;
        Ok(Some(tokens))
    }

    fn read_legacy_fields(&self) -> Result<LegacyFields, CliError> {
        let mut f = LegacyFields::default();
        for key in LegacyKey::ALL {
            let v = self.legacy.get(key.name())?.map(|s| s.expose().to_string());
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

    fn delete_legacy_fields(&self) -> Result<(), CliError> {
        for key in LegacyKey::ALL {
            self.legacy.delete(key.name())?;
        }
        Ok(())
    }

    /// Remove the session and any parked login, under both service names.
    pub fn clear(&self) -> Result<(), CliError> {
        self.store.delete(SESSION_KEY)?;
        self.store.delete(PENDING_KEY)?;
        self.legacy.delete(SESSION_KEY)?;
        self.legacy.delete(PENDING_KEY)?;
        self.delete_legacy_fields()
    }

    pub fn store_pending(&self, p: &PendingLogin) -> Result<(), CliError> {
        self.store.set_json(PENDING_KEY, p)
    }

    /// A parked login, if any (moved from the legacy service on read).
    pub fn pending(&self) -> Result<Option<PendingLogin>, CliError> {
        self.migrate_service()?;
        self.store.get_json(PENDING_KEY)
    }

    pub fn clear_pending(&self) -> Result<(), CliError> {
        self.store.delete(PENDING_KEY)?;
        self.legacy.delete(PENDING_KEY)?;
        Ok(())
    }
}

/// Refresh one cloud's token with its refresh token and persist the result.
/// `TokenExpired` (exit 3) when the refresh token itself has lapsed.
pub async fn refresh(
    sessions: &Sessions,
    tokens: &mut TokenSet,
    cloud: CloudType,
    verbose: bool,
) -> Result<(), CliError> {
    let (refresh_token, regional_url) = match cloud {
        CloudType::Kasa => (
            tokens.refresh_token.clone(),
            Some(tokens.regional_url.clone()),
        ),
        CloudType::Tapo => (
            tokens.tapo_refresh_token.clone(),
            tokens.tapo_regional_url.clone(),
        ),
    };
    let (Some(refresh_token), Some(regional_url)) = (refresh_token, regional_url) else {
        return Err(AppError::TokenExpired {
            message: format!(
                "{cloud} session expired and no refresh token is stored; run `tplc auth login`"
            ),
            error_code: None,
        }
        .into());
    };
    let api = TPLinkApi::new(
        Some(regional_url),
        verbose,
        Some(tokens.term_id.clone()),
        cloud,
    )?;
    let result = api.refresh_token(&refresh_token).await?;
    match cloud {
        CloudType::Kasa => {
            tokens.token = result.token;
            tokens.refresh_token = result.refresh_token;
            tokens.regional_url = result.regional_url;
        }
        CloudType::Tapo => {
            tokens.tapo_token = Some(result.token);
            tokens.tapo_refresh_token = result.refresh_token;
            tokens.tapo_regional_url = Some(result.regional_url);
        }
    }
    sessions.store(tokens)
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
            tapo_app_server_url: None,
            tapo_app_server_expires_at: None,
        };
        let blob = serde_json::to_string(&t).unwrap();
        let back: TokenSet = serde_json::from_str(&blob).unwrap();
        assert_eq!(back.token, "k");
        assert_eq!(back.tapo_token.as_deref(), Some("tt"));
        assert!(serde_json::from_str::<TokenSet>("{not json").is_err());
        // A blob written before the NBU cache fields existed still reads.
        let old = r#"{"token":"k","refresh_token":null,"username":"u","regional_url":"r","term_id":"t","tapo_token":null,"tapo_refresh_token":null,"tapo_regional_url":null}"#;
        let back: TokenSet = serde_json::from_str(old).unwrap();
        assert!(back.tapo_app_server_url.is_none());
    }

    /// The parked-login blob older builds wrote spelled the cloud as a bare
    /// lowercase string; `CloudType`'s serde form must keep reading it.
    #[test]
    fn pending_blob_reads_the_legacy_cloud_spelling() {
        let blob = r#"{"username":"u@example.com","term_id":"t","cloud":"tapo","kasa_token":"k","kasa_refresh_token":null,"kasa_regional_url":"https://example.com"}"#;
        let p: PendingLogin = serde_json::from_str(blob).unwrap();
        assert_eq!(p.cloud, CloudType::Tapo);
        assert_eq!(p.kasa_token.as_deref(), Some("k"));
    }

    #[test]
    fn service_name_follows_the_family_convention() {
        assert_eq!(Sessions::new().service(), "piekstra.tplc");
    }

    #[test]
    fn cloud_access_needs_a_tapo_session_for_tapo() {
        let t = TokenSet {
            token: "k".into(),
            refresh_token: None,
            username: "u".into(),
            regional_url: "r".into(),
            term_id: "t".into(),
            tapo_token: None,
            tapo_refresh_token: None,
            tapo_regional_url: None,
            tapo_app_server_url: None,
            tapo_app_server_expires_at: None,
        };
        assert!(t.cloud_access(CloudType::Kasa).is_ok());
        assert!(matches!(
            t.cloud_access(CloudType::Tapo),
            Err(AppError::NotAuthenticated)
        ));
        assert!(!t.has_tapo());
    }
}
