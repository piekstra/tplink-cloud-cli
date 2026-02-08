use dialoguer::{Input, Password};
use serde_json::json;

use crate::api::client::TPLinkApi;
use crate::api::cloud_type::CloudType;
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
                .with_prompt("TP-Link email")
                .interact_text()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
            let password: String = Password::new()
                .with_prompt("Password")
                .interact()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
            (username, password)
        }
    };

    // Login to Kasa cloud
    let mut kasa_api = TPLinkApi::new(None, config.verbose, None, CloudType::Kasa)?;

    let kasa_result = match kasa_api.login(&username, &password).await {
        Ok(result) => result,
        Err(AppError::MfaRequired { mfa_type: _, email }) => {
            eprintln!(
                "Kasa MFA verification required{}",
                email
                    .as_ref()
                    .map(|e| format!(" for {}", e))
                    .unwrap_or_default()
            );
            let mfa_code: String = Input::new()
                .with_prompt("Enter Kasa MFA code")
                .interact_text()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;

            kasa_api.verify_mfa(&username, &password, &mfa_code).await?
        }
        Err(e) => return Err(e),
    };

    // Login to Tapo cloud (best-effort, don't fail if Tapo login fails)
    let mut tapo_api = TPLinkApi::new(
        None,
        config.verbose,
        Some(kasa_api.term_id().to_string()),
        CloudType::Tapo,
    )?;

    let tapo_result = match tapo_api.login(&username, &password).await {
        Ok(result) => Some(result),
        Err(AppError::MfaRequired { mfa_type: _, email }) => {
            eprintln!(
                "Tapo MFA verification required{}",
                email
                    .as_ref()
                    .map(|e| format!(" for {}", e))
                    .unwrap_or_default()
            );
            let mfa_code: String = Input::new()
                .with_prompt("Enter Tapo MFA code")
                .interact_text()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;

            match tapo_api.verify_mfa(&username, &password, &mfa_code).await {
                Ok(result) => Some(result),
                Err(e) => {
                    if config.verbose {
                        eprintln!("Tapo MFA failed: {}", e);
                    }
                    None
                }
            }
        }
        Err(e) => {
            if config.verbose {
                eprintln!("Tapo login failed (non-fatal): {}", e);
            }
            None
        }
    };

    let tokens = TokenSet {
        token: kasa_result.token,
        refresh_token: kasa_result.refresh_token,
        username: username.clone(),
        regional_url: kasa_result.regional_url.clone(),
        term_id: kasa_api.term_id().to_string(),
        tapo_token: tapo_result.as_ref().map(|r| r.token.clone()),
        tapo_refresh_token: tapo_result.as_ref().and_then(|r| r.refresh_token.clone()),
        tapo_regional_url: tapo_result.as_ref().map(|r| r.regional_url.clone()),
    };

    keychain::store_tokens(&tokens)?;

    let mut status = json!({
        "status": "authenticated",
        "username": username,
        "kasa_regional_url": kasa_result.regional_url,
    });

    if let Some(ref tapo) = tapo_result {
        status["tapo_regional_url"] = json!(tapo.regional_url);
    } else {
        status["tapo"] = json!("unavailable");
    }

    print_json(&status);

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
                "kasa_regional_url": tokens.regional_url,
                "has_kasa_refresh_token": tokens.refresh_token.is_some(),
                "tapo_authenticated": tokens.tapo_token.is_some(),
                "has_tapo_refresh_token": tokens.tapo_refresh_token.is_some(),
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
