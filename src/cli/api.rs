//! `api <METHOD-NAME> [--params JSON] [--cloud kasa|tapo]` — call any
//! account-cloud method by name (`{"method": …, "params": …}` on the signed
//! v2 endpoint) and print the raw envelope. This is a cloud-RPC passthrough,
//! not the family's HTTP `api <VERB> <PATH>` form: TP-Link's cloud has no
//! REST paths to expose, only method names.

use pk_cli_core::CliError;
use serde_json::{json, Value};

use pk_cli_core::output::emit_one;

use super::emit::Ctx;
use crate::api::client::TPLinkApi;
use crate::api::cloud_type::CloudType;

#[derive(clap::Args, Debug, Clone)]
pub struct ApiArgs {
    /// Method name, e.g. getDeviceList, listDeviceGroups
    pub method: String,
    /// JSON object for the method's params
    #[arg(long)]
    pub params: Option<String>,
    /// Which cloud to call (else $TPLC_CLOUD, then `config set default_cloud`, then kasa)
    #[arg(long, value_enum, env = "TPLC_CLOUD")]
    pub cloud: Option<CloudType>,
}

/// Parse `--params` before anything is read from the keychain.
pub fn validate(args: &ApiArgs) -> Result<Option<Value>, CliError> {
    args.params
        .as_deref()
        .map(|p| {
            serde_json::from_str::<Value>(p)
                .map_err(|e| CliError::Usage(format!("--params is not valid JSON: {e}")))
        })
        .transpose()
}

pub async fn handle(ctx: &Ctx<'_>, args: &ApiArgs, params: Option<Value>) -> Result<(), CliError> {
    let cloud = args
        .cloud
        .or(ctx.cfg.default_cloud)
        .unwrap_or(CloudType::Kasa);
    let tokens = ctx.session()?;
    let (token, host) = tokens.cloud_access(cloud)?;
    let api = TPLinkApi::new(Some(host), ctx.verbose, Some(tokens.term_id.clone()), cloud)?;
    let resp = api.call_method(&token, &args.method, params).await?;
    emit_one(
        ctx.json,
        "api-response",
        json!({
            "cloud": cloud,
            "method": args.method,
            "error_code": resp.error_code,
            "msg": resp.msg,
            "result": resp.result,
        }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_must_be_json() {
        let bad = ApiArgs {
            method: "getDeviceList".into(),
            params: Some("{nope".into()),
            cloud: None,
        };
        assert_eq!(validate(&bad).unwrap_err().exit_code(), 2);
        let good = ApiArgs {
            params: Some(r#"{"a":1}"#.into()),
            ..bad
        };
        assert_eq!(validate(&good).unwrap(), Some(json!({"a": 1})));
    }
}
