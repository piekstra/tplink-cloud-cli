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

pub async fn handle_login(
    config: &RuntimeConfig,
    stdin: bool,
    username_flag: Option<&str>,
    mfa_code: Option<&str>,
) -> Result<(), AppError> {
    use std::io::{IsTerminal, Read};
    let interactive = std::io::stdin().is_terminal() && !stdin;
    let pending = keychain::get_pending()?;

    // Credentials: stdin (password) + --username, then env, then prompts.
    let (username, password) = if stdin {
        let mut pw = String::new();
        std::io::stdin()
            .read_to_string(&mut pw)
            .map_err(|e| AppError::InvalidInput(format!("reading password from stdin: {e}")))?;
        let pw = pw.trim_end_matches(['\r', '\n']).to_string();
        let user = username_flag
            .map(str::to_string)
            .or_else(|| {
                std::env::var("TPLC_USERNAME")
                    .ok()
                    .filter(|u| !u.is_empty())
            })
            .or_else(|| pending.as_ref().map(|p| p.username.clone()))
            .ok_or_else(|| {
                AppError::InvalidInput("--stdin needs --username (or TPLC_USERNAME)".into())
            })?;
        if pw.is_empty() {
            return Err(AppError::InvalidInput("no password on stdin".into()));
        }
        (user, pw)
    } else {
        match credentials_from_env() {
            Some((u, p)) => (u, p),
            None if interactive => {
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
            None => return Err(AppError::InvalidInput(
                "not a terminal: pass the password with --stdin and the account with --username"
                    .into(),
            )),
        }
    };

    // Resume: a parked login keeps its terminal id, which the emailed code is bound to.
    let resume = pending.filter(|p| p.username == username);
    let term_id = resume.as_ref().map(|p| p.term_id.clone());
    let mut kasa_api = TPLinkApi::new(None, config.verbose, term_id, CloudType::Kasa)?;

    // A code on the command line is only meaningful for the cloud that asked.
    let mut code_for_kasa = mfa_code.filter(|_| resume.as_ref().is_none_or(|p| p.cloud == "kasa"));
    let mut code_for_tapo = mfa_code.filter(|_| resume.as_ref().is_some_and(|p| p.cloud == "tapo"));

    let kasa_result = match &resume {
        Some(p) if p.cloud == "tapo" && p.kasa_token.is_some() => crate::api::client::LoginResult {
            token: p.kasa_token.clone().unwrap_or_default(),
            refresh_token: p.kasa_refresh_token.clone(),
            regional_url: p.kasa_regional_url.clone().unwrap_or_default(),
        },
        _ => {
            let attempt = match code_for_kasa.take() {
                Some(code) => kasa_api.verify_mfa(&username, &password, code).await,
                None => kasa_api.login(&username, &password).await,
            };
            match attempt {
                Ok(result) => result,
                Err(AppError::MfaRequired { mfa_type: _, email }) => {
                    if interactive {
                        eprintln!(
                            "Kasa MFA verification required{}",
                            email
                                .as_ref()
                                .map(|e| format!(" for {}", e))
                                .unwrap_or_default()
                        );
                        let code: String = Input::new()
                            .with_prompt("Enter Kasa MFA code")
                            .interact_text()
                            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
                        kasa_api.verify_mfa(&username, &password, &code).await?
                    } else {
                        keychain::store_pending(&keychain::PendingLogin {
                            username: username.clone(),
                            term_id: kasa_api.term_id().to_string(),
                            cloud: "kasa".into(),
                            kasa_token: None,
                            kasa_refresh_token: None,
                            kasa_regional_url: None,
                        })?;
                        print_json(&json!({
                            "status": "mfa_required",
                            "cloud": "kasa",
                            "email": email,
                            "next": "re-run with --mfa-code <CODE> from the email",
                        }));
                        return Ok(());
                    }
                }
                Err(e) => return Err(e),
            }
        }
    };

    // Tapo, same terminal id (best-effort: a missing Tapo session is not fatal).
    let mut tapo_api = TPLinkApi::new(
        None,
        config.verbose,
        Some(kasa_api.term_id().to_string()),
        CloudType::Tapo,
    )?;
    let tapo_attempt = match code_for_tapo.take() {
        Some(code) => tapo_api.verify_mfa(&username, &password, code).await,
        None => tapo_api.login(&username, &password).await,
    };
    let tapo_result = match tapo_attempt {
        Ok(result) => Some(result),
        Err(AppError::MfaRequired { mfa_type: _, email }) => {
            if interactive {
                eprintln!(
                    "Tapo MFA verification required{}",
                    email
                        .as_ref()
                        .map(|e| format!(" for {}", e))
                        .unwrap_or_default()
                );
                let code: String = Input::new()
                    .with_prompt("Enter Tapo MFA code")
                    .interact_text()
                    .map_err(|e| AppError::InvalidInput(e.to_string()))?;
                match tapo_api.verify_mfa(&username, &password, &code).await {
                    Ok(result) => Some(result),
                    Err(e) => {
                        if config.verbose {
                            eprintln!("Tapo MFA failed: {}", e);
                        }
                        None
                    }
                }
            } else {
                keychain::store_pending(&keychain::PendingLogin {
                    username: username.clone(),
                    term_id: kasa_api.term_id().to_string(),
                    cloud: "tapo".into(),
                    kasa_token: Some(kasa_result.token.clone()),
                    kasa_refresh_token: kasa_result.refresh_token.clone(),
                    kasa_regional_url: Some(kasa_result.regional_url.clone()),
                })?;
                print_json(&json!({
                    "status": "mfa_required",
                    "cloud": "tapo",
                    "email": email,
                    "next": "re-run with --mfa-code <CODE> from the email",
                }));
                return Ok(());
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
    keychain::clear_pending()?;

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
