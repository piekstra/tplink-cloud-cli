use std::env;

use crate::api::client::TPLinkApi;
use crate::auth::keychain;
use crate::auth::token::TokenSet;
use crate::error::AppError;

pub struct AuthContext {
    pub token: String,
    pub refresh_token: Option<String>,
    pub regional_url: String,
    pub term_id: String,
    pub username: String,
}

impl AuthContext {
    pub fn to_token_set(&self) -> TokenSet {
        TokenSet {
            token: self.token.clone(),
            refresh_token: self.refresh_token.clone(),
            username: self.username.clone(),
            regional_url: self.regional_url.clone(),
            term_id: self.term_id.clone(),
        }
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
    })
}

/// Attempt to refresh the token and update keychain.
pub async fn refresh_auth(auth: &mut AuthContext, verbose: bool) -> Result<(), AppError> {
    let refresh_token = auth
        .refresh_token
        .as_deref()
        .ok_or(AppError::NotAuthenticated)?;

    let api = TPLinkApi::new(
        Some(auth.regional_url.clone()),
        verbose,
        Some(auth.term_id.clone()),
    )?;

    let result = api.refresh_token(refresh_token).await?;

    auth.token = result.token;
    auth.refresh_token = result.refresh_token;
    auth.regional_url = result.regional_url;

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
