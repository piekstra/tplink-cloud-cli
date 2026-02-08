use std::env;

use crate::api::client::TPLinkApi;
use crate::api::cloud_type::CloudType;
use crate::auth::keychain;
use crate::auth::token::TokenSet;
use crate::error::AppError;

pub struct AuthContext {
    pub token: String,
    pub refresh_token: Option<String>,
    pub regional_url: String,
    pub term_id: String,
    pub username: String,
    pub tapo_token: Option<String>,
    pub tapo_refresh_token: Option<String>,
    pub tapo_regional_url: Option<String>,
}

impl AuthContext {
    pub fn to_token_set(&self) -> TokenSet {
        TokenSet {
            token: self.token.clone(),
            refresh_token: self.refresh_token.clone(),
            username: self.username.clone(),
            regional_url: self.regional_url.clone(),
            term_id: self.term_id.clone(),
            tapo_token: self.tapo_token.clone(),
            tapo_refresh_token: self.tapo_refresh_token.clone(),
            tapo_regional_url: self.tapo_regional_url.clone(),
        }
    }

    pub fn has_tapo(&self) -> bool {
        self.tapo_token.as_ref().is_some_and(|t| !t.is_empty())
    }
}

/// Get stored authentication context, auto-refreshing if needed.
pub async fn get_auth_context(_verbose: bool) -> Result<AuthContext, AppError> {
    let tokens = keychain::get_tokens()?.ok_or(AppError::NotAuthenticated)?;

    if tokens.token.is_empty() {
        return Err(AppError::NotAuthenticated);
    }

    Ok(AuthContext {
        token: tokens.token,
        refresh_token: tokens.refresh_token,
        regional_url: tokens.regional_url,
        term_id: tokens.term_id,
        username: tokens.username,
        tapo_token: tokens.tapo_token,
        tapo_refresh_token: tokens.tapo_refresh_token,
        tapo_regional_url: tokens.tapo_regional_url,
    })
}

/// Attempt to refresh the Kasa token and update keychain.
pub async fn refresh_auth(auth: &mut AuthContext, verbose: bool) -> Result<(), AppError> {
    let refresh_token = auth
        .refresh_token
        .as_deref()
        .ok_or(AppError::NotAuthenticated)?;

    let api = TPLinkApi::new(
        Some(auth.regional_url.clone()),
        verbose,
        Some(auth.term_id.clone()),
        CloudType::Kasa,
    )?;

    let result = api.refresh_token(refresh_token).await?;

    auth.token = result.token;
    auth.refresh_token = result.refresh_token;
    auth.regional_url = result.regional_url;

    keychain::store_tokens(&auth.to_token_set())?;

    Ok(())
}

/// Attempt to refresh the Tapo token and update keychain.
pub async fn refresh_tapo_auth(auth: &mut AuthContext, verbose: bool) -> Result<(), AppError> {
    let refresh_token = auth
        .tapo_refresh_token
        .as_deref()
        .ok_or(AppError::NotAuthenticated)?;

    let regional_url = auth
        .tapo_regional_url
        .as_deref()
        .ok_or(AppError::NotAuthenticated)?;

    let api = TPLinkApi::new(
        Some(regional_url.to_string()),
        verbose,
        Some(auth.term_id.clone()),
        CloudType::Tapo,
    )?;

    let result = api.refresh_token(refresh_token).await?;

    auth.tapo_token = Some(result.token);
    auth.tapo_refresh_token = result.refresh_token;
    auth.tapo_regional_url = Some(result.regional_url);

    keychain::store_tokens(&auth.to_token_set())?;

    Ok(())
}

/// Get credentials from env vars for login, or None if not set.
pub fn credentials_from_env() -> Option<(String, String)> {
    let username = env::var("TPLC_USERNAME").ok()?;
    let password = env::var("TPLC_PASSWORD").ok()?;
    if username.is_empty() || password.is_empty() {
        return None;
    }
    Some((username, password))
}
