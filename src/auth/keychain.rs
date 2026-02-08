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

pub fn store_tokens(tokens: &TokenSet) -> Result<(), AppError> {
    set_value("token", &tokens.token)?;
    if let Some(ref rt) = tokens.refresh_token {
        set_value("refresh_token", rt)?;
    }
    set_value("username", &tokens.username)?;
    set_value("regional_url", &tokens.regional_url)?;
    set_value("term_id", &tokens.term_id)?;

    // Tapo tokens
    if let Some(ref tt) = tokens.tapo_token {
        set_value("tapo_token", tt)?;
    }
    if let Some(ref trt) = tokens.tapo_refresh_token {
        set_value("tapo_refresh_token", trt)?;
    }
    if let Some(ref tru) = tokens.tapo_regional_url {
        set_value("tapo_regional_url", tru)?;
    }

    Ok(())
}

pub fn get_tokens() -> Result<Option<TokenSet>, AppError> {
    let token = match get_value("token")? {
        Some(t) => t,
        None => return Ok(None),
    };
    let username = get_value("username")?.unwrap_or_default();
    let regional_url = get_value("regional_url")?.unwrap_or_default();
    let term_id = get_value("term_id")?.unwrap_or_default();
    let refresh_token = get_value("refresh_token")?;
    let tapo_token = get_value("tapo_token")?;
    let tapo_refresh_token = get_value("tapo_refresh_token")?;
    let tapo_regional_url = get_value("tapo_regional_url")?;

    Ok(Some(TokenSet {
        token,
        refresh_token,
        username,
        regional_url,
        term_id,
        tapo_token,
        tapo_refresh_token,
        tapo_regional_url,
    }))
}

pub fn clear_tokens() -> Result<(), AppError> {
    delete_value("token")?;
    delete_value("refresh_token")?;
    delete_value("username")?;
    delete_value("regional_url")?;
    delete_value("term_id")?;
    delete_value("tapo_token")?;
    delete_value("tapo_refresh_token")?;
    delete_value("tapo_regional_url")?;
    Ok(())
}
