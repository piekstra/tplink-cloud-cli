use serde_json::json;

use crate::api::client::TPLinkApi;
use crate::api::cloud_type::CloudType;
use crate::auth::credentials;
use crate::cli::output::print_json;
use crate::config::RuntimeConfig;
use crate::error::AppError;

pub async fn handle(
    method: &str,
    params: Option<&str>,
    cloud: &str,
    config: &RuntimeConfig,
) -> Result<(), AppError> {
    let params = params
        .map(serde_json::from_str::<serde_json::Value>)
        .transpose()
        .map_err(|e| AppError::InvalidInput(format!("--params is not valid JSON: {e}")))?;
    let cloud_type = match cloud.to_lowercase().as_str() {
        "kasa" => CloudType::Kasa,
        "tapo" => CloudType::Tapo,
        other => {
            return Err(AppError::InvalidInput(format!(
                "--cloud must be kasa or tapo, got `{other}`"
            )))
        }
    };
    let auth = credentials::get_auth_context(config.verbose).await?;
    let (host, token) = match cloud_type {
        CloudType::Kasa => (auth.regional_url.clone(), auth.token.clone()),
        CloudType::Tapo => (
            auth.tapo_regional_url
                .clone()
                .ok_or(AppError::NotAuthenticated)?,
            auth.tapo_token.clone().ok_or(AppError::NotAuthenticated)?,
        ),
    };
    let api = TPLinkApi::new(Some(host), config.verbose, Some(auth.term_id.clone()), cloud_type)?;
    let resp = api.call_method(&token, method, params).await?;
    print_json(&json!({
        "error_code": resp.error_code,
        "msg": resp.msg,
        "result": resp.result,
    }));
    Ok(())
}
