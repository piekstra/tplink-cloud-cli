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
//! [`Sessions`] talks to the keychain through the [`SecretItems`] seam:
//! `CredentialStore` in the binary, an in-memory store in the tests, so the
//! migration order is asserted without a keychain (an ad-hoc-signed test
//! binary reading a real item is a macOS prompt per run).
//!
//! No password is ever stored. `auth login` exchanges it for tokens and
//! forgets it.

use std::future::Future;

use pk_cli_core::CliError;
use pk_cli_secrets::CredentialStore;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

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

/// What this module needs from a keychain service: string items (the
/// per-field legacy layout), typed JSON items (the session, the parked
/// login), and the family's item migration. `CredentialStore` implements it
/// by delegation; tests implement it in memory.
pub trait SecretItems {
    fn service(&self) -> &str;
    /// A raw string item, `None` when absent.
    fn get(&self, key: &str) -> Result<Option<String>, CliError>;
    /// A typed JSON item; a present-but-unparseable item is an error.
    fn get_json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, CliError>;
    fn set_json<T: Serialize>(&self, key: &str, value: &T) -> Result<(), CliError>;
    /// `true` if something was removed.
    fn delete(&self, key: &str) -> Result<bool, CliError>;
    /// Per `(old, new)` pair: read from `legacy`, write here if absent,
    /// delete from `legacy`. Returns how many were copied.
    fn migrate_from(&self, legacy: &Self, keys: &[(&str, &str)]) -> Result<usize, CliError>;
}

impl SecretItems for CredentialStore {
    fn service(&self) -> &str {
        CredentialStore::service(self)
    }
    fn get(&self, key: &str) -> Result<Option<String>, CliError> {
        Ok(CredentialStore::get(self, key)?.map(|s| s.expose().to_string()))
    }
    fn get_json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, CliError> {
        CredentialStore::get_json(self, key)
    }
    fn set_json<T: Serialize>(&self, key: &str, value: &T) -> Result<(), CliError> {
        CredentialStore::set_json(self, key, value)
    }
    fn delete(&self, key: &str) -> Result<bool, CliError> {
        CredentialStore::delete(self, key)
    }
    fn migrate_from(&self, legacy: &Self, keys: &[(&str, &str)]) -> Result<usize, CliError> {
        CredentialStore::migrate_from(self, legacy, keys)
    }
}

/// The session store: the family service plus the legacy one it migrates
/// from. `Sessions::new()` binds the OS keychain; `with_stores` is the seam.
pub struct Sessions<S: SecretItems = CredentialStore> {
    store: S,
    legacy: S,
}

impl Default for Sessions {
    fn default() -> Self {
        Self::new()
    }
}

impl Sessions {
    /// The real thing: `piekstra.tplc`, migrating from `tplc`.
    pub fn new() -> Self {
        Sessions::with_stores(
            CredentialStore::for_binary(BIN),
            CredentialStore::new(LEGACY_SERVICE),
        )
    }
}

impl<S: SecretItems> Sessions<S> {
    pub fn with_stores(store: S, legacy: S) -> Self {
        Sessions { store, legacy }
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

    /// The stored session, migrating any legacy layout on the way. Order:
    /// move the legacy two-item layout, read the family item, and only if
    /// that is absent read the per-field legacy items, write them as one
    /// item, then delete them. A present-but-unparseable item is an error
    /// (from `get_json`), not "never logged in": one blob carries everything,
    /// so a swallowed parse failure would read as an unexplained logout.
    pub fn load(&self) -> Result<Option<TokenSet>, CliError> {
        self.migrate_service()?;
        if let Some(tokens) = self.store.get_json::<TokenSet>(SESSION_KEY)? {
            return Ok(Some(tokens));
        }
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
            let v = self.legacy.get(key.name())?;
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

/// The one policy for calling a cloud with a stored token: run `op` with the
/// current tokens; if it fails with `TokenExpired`, refresh that cloud's
/// token **once** (persisting it) and run `op` once more with the new
/// tokens. Any other error, and a second `TokenExpired`, are returned as
/// they are; a failed refresh reports the refresh's own error (exit 3) so
/// the user is sent to `auth login` rather than to a retry. Never loops.
///
/// `op` builds its request from the `TokenSet` it is handed (cloning the
/// token/URL it needs into the future), so the retry sees the refreshed
/// values without the caller threading them through.
pub async fn with_refresh<T, F, Fut>(
    sessions: &Sessions,
    tokens: &mut TokenSet,
    cloud: CloudType,
    verbose: bool,
    op: F,
) -> Result<T, CliError>
where
    F: Fn(&TokenSet) -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    match op(tokens).await {
        Err(AppError::TokenExpired { .. }) => {
            refresh(sessions, tokens, cloud, verbose).await?;
            op(tokens).await.map_err(Into::into)
        }
        other => other.map_err(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    fn tokens(token: &str) -> TokenSet {
        TokenSet {
            token: token.into(),
            refresh_token: None,
            username: "user@example.com".into(),
            regional_url: "https://n-use1-wap.tplinkcloud.com".into(),
            term_id: "00000000-0000-4000-8000-000000000000".into(),
            tapo_token: None,
            tapo_refresh_token: None,
            tapo_regional_url: None,
            tapo_app_server_url: None,
            tapo_app_server_expires_at: None,
        }
    }

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
        let mut t = tokens("k");
        t.tapo_token = Some("tt".into());
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
        let t = tokens("k");
        assert!(t.cloud_access(CloudType::Kasa).is_ok());
        assert!(matches!(
            t.cloud_access(CloudType::Tapo),
            Err(AppError::NotAuthenticated)
        ));
        assert!(!t.has_tapo());
    }

    // ---- the migration order, against an in-memory keychain ----

    /// An in-memory keychain service. `fail_writes` simulates a write that
    /// errors, for the "a failure midway leaves the old layout" rail.
    struct MemStore {
        service: String,
        items: RefCell<HashMap<String, String>>,
        fail_writes: Cell<bool>,
    }

    impl MemStore {
        fn new(service: &str) -> Self {
            MemStore {
                service: service.into(),
                items: RefCell::new(HashMap::new()),
                fail_writes: Cell::new(false),
            }
        }
        fn with(self, key: &str, value: &str) -> Self {
            self.items.borrow_mut().insert(key.into(), value.into());
            self
        }
        fn keys(&self) -> Vec<String> {
            let mut k: Vec<String> = self.items.borrow().keys().cloned().collect();
            k.sort();
            k
        }
        fn raw(&self, key: &str) -> Option<String> {
            self.items.borrow().get(key).cloned()
        }
    }

    impl SecretItems for MemStore {
        fn service(&self) -> &str {
            &self.service
        }
        fn get(&self, key: &str) -> Result<Option<String>, CliError> {
            Ok(self.raw(key))
        }
        fn get_json<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, CliError> {
            self.raw(key)
                .map(|s| {
                    serde_json::from_str(&s)
                        .map_err(|e| CliError::Keychain(format!("item `{key}` unreadable: {e}")))
                })
                .transpose()
        }
        fn set_json<T: Serialize>(&self, key: &str, value: &T) -> Result<(), CliError> {
            if self.fail_writes.get() {
                return Err(CliError::Keychain("simulated write failure".into()));
            }
            let s = serde_json::to_string(value).unwrap();
            self.items.borrow_mut().insert(key.into(), s);
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<bool, CliError> {
            Ok(self.items.borrow_mut().remove(key).is_some())
        }
        fn migrate_from(&self, legacy: &Self, keys: &[(&str, &str)]) -> Result<usize, CliError> {
            // The shared order: read old → write new if absent → delete old.
            let mut moved = 0;
            for (old, new) in keys {
                let Some(v) = legacy.raw(old) else { continue };
                if self.raw(new).is_none() {
                    if self.fail_writes.get() {
                        return Err(CliError::Keychain("simulated write failure".into()));
                    }
                    self.items.borrow_mut().insert((*new).into(), v);
                    moved += 1;
                }
                legacy.delete(old)?;
            }
            Ok(moved)
        }
    }

    fn blob(token: &str) -> String {
        serde_json::to_string(&tokens(token)).unwrap()
    }

    #[test]
    fn a_family_item_wins_over_every_legacy_layout() {
        let store = MemStore::new("piekstra.tplc").with(SESSION_KEY, &blob("new"));
        let legacy = MemStore::new("tplc")
            .with(SESSION_KEY, &blob("old-json"))
            .with("token", "old-field");
        let s = Sessions::with_stores(store, legacy);
        assert_eq!(s.load().unwrap().unwrap().token, "new");
        // The legacy JSON item is retired; the per-field items were never read.
        assert!(s.legacy.raw(SESSION_KEY).is_none());
        assert_eq!(s.legacy.keys(), ["token"]);
        assert_eq!(s.store.keys(), [SESSION_KEY]);
    }

    #[test]
    fn a_legacy_json_item_is_moved_then_retired() {
        let s = Sessions::with_stores(
            MemStore::new("piekstra.tplc"),
            MemStore::new("tplc").with(SESSION_KEY, &blob("moved")),
        );
        assert_eq!(s.load().unwrap().unwrap().token, "moved");
        assert_eq!(s.store.keys(), [SESSION_KEY]);
        assert!(s.legacy.keys().is_empty());
        // Converges: a second load reads the family item only.
        assert_eq!(s.load().unwrap().unwrap().token, "moved");
    }

    #[test]
    fn per_field_legacy_items_are_written_as_one_item_before_they_are_deleted() {
        let legacy = MemStore::new("tplc")
            .with("token", "kasa-token")
            .with("refresh_token", "kasa-refresh")
            .with("username", "user@example.com")
            .with("regional_url", "https://n-use1-wap.tplinkcloud.com")
            .with("term_id", "t")
            .with("tapo_token", "tapo-token")
            .with("tapo_refresh_token", "tapo-refresh")
            .with("tapo_regional_url", "https://n-use1-wap-gw.tplinkcloud.com");
        let s = Sessions::with_stores(MemStore::new("piekstra.tplc"), legacy);
        let t = s.load().unwrap().expect("eight items are a session");
        assert_eq!(t.token, "kasa-token");
        assert_eq!(t.tapo_token.as_deref(), Some("tapo-token"));
        assert_eq!(s.store.keys(), [SESSION_KEY], "one item, not eight");
        assert!(s.legacy.keys().is_empty(), "the eight are gone");
        let stored: TokenSet = s.store.get_json(SESSION_KEY).unwrap().unwrap();
        assert_eq!(stored.username, "user@example.com");
    }

    #[test]
    fn a_failed_write_leaves_the_legacy_layout_intact() {
        let store = MemStore::new("piekstra.tplc");
        store.fail_writes.set(true);
        let legacy = MemStore::new("tplc")
            .with("token", "kasa-token")
            .with("username", "user@example.com");
        let s = Sessions::with_stores(store, legacy);
        assert!(s.load().is_err());
        assert_eq!(s.legacy.keys(), ["token", "username"], "nothing deleted");
        assert!(s.store.keys().is_empty());
        // The next run, with a working keychain, completes the migration.
        s.store.fail_writes.set(false);
        assert_eq!(s.load().unwrap().unwrap().token, "kasa-token");
        assert!(s.legacy.keys().is_empty());
    }

    #[test]
    fn a_failed_service_move_leaves_the_legacy_item_for_the_next_run() {
        let store = MemStore::new("piekstra.tplc");
        store.fail_writes.set(true);
        let legacy = MemStore::new("tplc").with(SESSION_KEY, &blob("old"));
        let s = Sessions::with_stores(store, legacy);
        assert!(s.load().is_err());
        assert_eq!(s.legacy.raw(SESSION_KEY), Some(blob("old")));
    }

    #[test]
    fn no_session_anywhere_is_none_not_an_error() {
        let s = Sessions::with_stores(MemStore::new("piekstra.tplc"), MemStore::new("tplc"));
        assert!(s.load().unwrap().is_none());
        assert!(s.pending().unwrap().is_none());
    }

    #[test]
    fn an_unreadable_family_item_is_an_error_not_a_logout() {
        let s = Sessions::with_stores(
            MemStore::new("piekstra.tplc").with(SESSION_KEY, "{not json"),
            MemStore::new("tplc").with(SESSION_KEY, &blob("old")),
        );
        let err = s.load().unwrap_err();
        assert!(matches!(err, CliError::Keychain(_)), "{err}");
    }

    #[test]
    fn clear_empties_both_services() {
        let s = Sessions::with_stores(
            MemStore::new("piekstra.tplc")
                .with(SESSION_KEY, &blob("new"))
                .with(PENDING_KEY, "{}"),
            MemStore::new("tplc")
                .with(SESSION_KEY, &blob("old"))
                .with("token", "x")
                .with("tapo_token", "y"),
        );
        s.clear().unwrap();
        assert!(s.store.keys().is_empty());
        assert!(s.legacy.keys().is_empty());
    }

    #[test]
    fn a_parked_login_moves_with_the_session() {
        let park = PendingLogin {
            username: "user@example.com".into(),
            term_id: "t".into(),
            cloud: CloudType::Kasa,
            kasa_token: None,
            kasa_refresh_token: None,
            kasa_regional_url: None,
        };
        let s = Sessions::with_stores(
            MemStore::new("piekstra.tplc"),
            MemStore::new("tplc").with(PENDING_KEY, &serde_json::to_string(&park).unwrap()),
        );
        assert_eq!(s.pending().unwrap().unwrap().cloud, CloudType::Kasa);
        assert_eq!(s.store.keys(), [PENDING_KEY]);
        assert!(s.legacy.keys().is_empty());
        s.clear_pending().unwrap();
        assert!(s.pending().unwrap().is_none());
    }

    // ---- the refresh-once policy (no network: the refresh path stops at
    //      "no refresh token stored" before any request) ----

    fn block_on<T>(f: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn with_refresh_passes_success_and_other_errors_through_untouched() {
        let sessions = Sessions::new();
        let mut t = tokens("k");
        let calls = Cell::new(0);
        let ok = block_on(with_refresh(
            &sessions,
            &mut t,
            CloudType::Kasa,
            false,
            |_| {
                calls.set(calls.get() + 1);
                async { Ok::<_, AppError>("data") }
            },
        ))
        .unwrap();
        assert_eq!(ok, "data");
        assert_eq!(calls.get(), 1);

        calls.set(0);
        let err = block_on(with_refresh(
            &sessions,
            &mut t,
            CloudType::Kasa,
            false,
            |_| {
                calls.set(calls.get() + 1);
                async {
                    Err::<(), _>(AppError::Api {
                        message: "boom".into(),
                        error_code: Some(-1),
                    })
                }
            },
        ))
        .unwrap_err();
        assert_eq!(err.exit_code(), 5);
        assert_eq!(calls.get(), 1, "a non-auth error is never retried");
    }

    #[test]
    fn with_refresh_reports_the_refresh_failure_and_does_not_retry() {
        let sessions = Sessions::new();
        let mut t = tokens("k"); // no refresh token: refresh fails before any request
        let calls = Cell::new(0);
        let err = block_on(with_refresh(
            &sessions,
            &mut t,
            CloudType::Kasa,
            false,
            |_| {
                calls.set(calls.get() + 1);
                async {
                    Err::<(), _>(AppError::TokenExpired {
                        message: "expired".into(),
                        error_code: Some(-20651),
                    })
                }
            },
        ))
        .unwrap_err();
        assert_eq!(err.exit_code(), 3);
        assert!(err.to_string().contains("auth login"), "{err}");
        assert_eq!(calls.get(), 1, "no retry once the refresh failed");
    }
}
