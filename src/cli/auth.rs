use dialoguer::{Input, Password};
use serde_json::json;

use crate::api::client::TPLinkApi;
use crate::auth::credentials::credentials_from_env;
use crate::auth::keychain;
use crate::auth::token::TokenSet;
use crate::cli::output::print_json;
use crate::config::RuntimeConfig;
use crate::error::AppError;

pub async fn handle_login(config: &RuntimeConfig) -> Result<(), AppError> {
    let (username, password) = match credentials_from_env() {
        Some((u, p)) => (u, p),
        None => {
            let username: String = Input::new()
                .with_prompt("TP-Link/Kasa email")
                .interact_text()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
            let password: String = Password::new()
                .with_prompt("Password")
                .interact()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
            (username, password)
        }
    };

    let mut api = TPLinkApi::new(None, config.verbose, None)?;

    let result = match api.login(&username, &password).await {
        Ok(result) => result,
        Err(AppError::MfaRequired { mfa_type: _, email }) => {
            eprintln!(
                "MFA verification required{}",
                email
                    .as_ref()
                    .map(|e| format!(" for {}", e))
                    .unwrap_or_default()
            );
            let mfa_code: String = Input::new()
                .with_prompt("Enter MFA code")
                .interact_text()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;

            api.verify_mfa(&username, &password, &mfa_code).await?
        }
        Err(e) => return Err(e),
    };

    let tokens = TokenSet {
        token: result.token,
        refresh_token: result.refresh_token,
        username: username.clone(),
        regional_url: result.regional_url.clone(),
        term_id: api.term_id().to_string(),
    };

    keychain::store_tokens(&tokens)?;

    print_json(&json!({
        "status": "authenticated",
        "username": username,
        "regional_url": result.regional_url,
    }));

    Ok(())
}

pub async fn handle_logout(_config: &RuntimeConfig) -> Result<(), AppError> {
    keychain::clear_tokens()?;
    print_json(&json!({"status": "logged_out"}));
    Ok(())
}

pub async fn handle_status(_config: &RuntimeConfig) -> Result<(), AppError> {
    match keychain::get_tokens()? {
        Some(tokens) => {
            print_json(&json!({
                "status": "authenticated",
                "username": tokens.username,
                "regional_url": tokens.regional_url,
                "has_refresh_token": tokens.refresh_token.is_some(),
            }));
        }
        None => {
            print_json(&json!({
                "status": "not_authenticated",
            }));
        }
    }
    Ok(())
}
