//! Newer Kasa hardware (`SMART.KASA*`, e.g. an HS200 v5 switch) through its
//! thing shadow on the NBU app-server, the route the current Kasa app uses.
//! The cloud passthrough answers "Device is offline" (-20571) for these
//! devices whether or not they are connected, so `power` and `devices get`
//! come here for them instead.

use std::future::Future;

use pk_cli_core::CliError;
use serde_json::{json, Value};

use super::emit::Ctx;
use crate::api::client::TPLinkApi;
use crate::api::cloud_type::CloudType;
use crate::api::nbu::{self, NbuClient};
use crate::error::AppError;
use crate::resolve::Listed;
use crate::session::{self, TokenSet};

/// One device's shadow: what it last reported and the version a change
/// must build on.
#[derive(Debug, Clone)]
pub struct Shadow {
    pub raw: Value,
}

impl Shadow {
    pub fn version(&self) -> i64 {
        self.raw.get("version").and_then(Value::as_i64).unwrap_or(0)
    }

    pub fn reported(&self) -> Option<&Value> {
        self.raw.get("state").and_then(|s| s.get("reported"))
    }

    /// The relay state the device last reported.
    pub fn is_on(&self) -> Option<bool> {
        on_of(self.reported()?)
    }
}

/// `on` as the shadow carries it: a boolean, or 0/1 on some firmware.
fn on_of(state: &Value) -> Option<bool> {
    match state.get("on")? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|v| v != 0),
        _ => None,
    }
}

/// Shadows are read with the Kasa login, presenting the Kasa app's
/// identity: these devices are registered in the Kasa app, and the same
/// call with the Tapo login is refused (HTTP 403).
const SHADOW_LOGIN: CloudType = CloudType::Kasa;

/// A shadow session: the tokens and an NBU client presenting the Kasa
/// app's identity. Calls refresh the Kasa token once if it has lapsed.
struct Shadows<'a> {
    ctx: &'a Ctx<'a>,
    tokens: TokenSet,
    client: NbuClient,
}

impl<'a> Shadows<'a> {
    async fn call<T, F, Fut>(&mut self, op: F) -> Result<T, CliError>
    where
        F: Fn(NbuClient) -> Fut,
        Fut: Future<Output = Result<T, AppError>>,
    {
        let client = &self.client;
        session::with_refresh(
            self.ctx.sessions,
            &mut self.tokens,
            SHADOW_LOGIN,
            self.ctx.verbose,
            |t| {
                let token = t
                    .cloud_access(SHADOW_LOGIN)
                    .map(|(k, _)| k)
                    .unwrap_or_default();
                op(client.with_token(&token))
            },
        )
        .await
    }
}

/// The app-server host for the Kasa login, from `getAppServiceUrl` on the
/// Kasa cloud. Not cached: the session item is only written by sign-ins
/// and refreshes.
async fn kasa_app_server(ctx: &Ctx<'_>, tokens: &mut TokenSet) -> Result<String, CliError> {
    let verbose = ctx.verbose;
    session::with_refresh(ctx.sessions, tokens, SHADOW_LOGIN, verbose, |t| {
        let access = t.cloud_access(SHADOW_LOGIN);
        let term_id = t.term_id.clone();
        async move {
            let (token, regional_url) = access?;
            let api = TPLinkApi::new(Some(regional_url), verbose, Some(term_id), SHADOW_LOGIN)?;
            api.get_app_service_url(&token, nbu::APP_SERVER_SERVICE_ID)
                .await
        }
    })
    .await
}

async fn connect<'a>(ctx: &'a Ctx<'a>) -> Result<Shadows<'a>, CliError> {
    let mut tokens = ctx.session()?;
    let base = kasa_app_server(ctx, &mut tokens).await?;
    let (token, _) = tokens.cloud_access(SHADOW_LOGIN)?;
    let client = NbuClient::new_as(
        &base,
        &token,
        &tokens.term_id,
        ctx.verbose,
        SHADOW_LOGIN.app_type(),
        SHADOW_LOGIN.app_version(),
    )?;
    Ok(Shadows {
        ctx,
        tokens,
        client,
    })
}

/// The shadows of `ids`, in the cloud's order; a device without one is missing.
async fn read_many(tapo: &mut Shadows<'_>, ids: &[String]) -> Result<Vec<Shadow>, CliError> {
    let ids = ids.to_vec();
    let raw = tapo
        .call(|c| {
            let ids = ids.clone();
            async move { c.shadows(&ids).await }
        })
        .await?;
    Ok(raw.into_iter().map(|raw| Shadow { raw }).collect())
}

async fn read_one(tapo: &mut Shadows<'_>, id: &str) -> Result<Shadow, CliError> {
    read_many(tapo, &[id.to_string()])
        .await?
        .into_iter()
        .find(|s| s.raw.get("thingName").and_then(Value::as_str).unwrap_or(id) == id)
        .ok_or_else(|| CliError::Upstream(format!("the cloud has no state for device {id}")))
}

/// Mark shadow devices whose shadow carries a reported on/off state as
/// online. The getDeviceList status comes from the passthrough route, which
/// calls every `SMART.KASA*` device offline. The shadow has no connection
/// timestamp, so this is "the cloud has its state", not a live ping.
/// Best-effort: on failure the list keeps the cloud's status.
pub async fn mark_reachable(ctx: &Ctx<'_>, devices: &mut [Listed]) {
    let ids: Vec<String> = devices
        .iter()
        .filter(|d| d.info.uses_shadow() && d.child_id.is_none())
        .map(|d| d.info.id().to_string())
        .collect();
    if ids.is_empty() {
        return;
    }
    let read = async {
        let mut s = connect(ctx).await?;
        read_many(&mut s, &ids).await
    };
    match read.await {
        Ok(shadows) => {
            for shadow in shadows.iter().filter(|s| s.is_on().is_some()) {
                let Some(name) = shadow.raw.get("thingName").and_then(Value::as_str) else {
                    continue;
                };
                for d in devices.iter_mut().filter(|d| d.info.id() == name) {
                    d.info.status = Some(1);
                }
            }
        }
        Err(e) => {
            if ctx.verbose {
                eprintln!("Shadow states could not be read: {e} (non-fatal)");
            }
        }
    }
}

/// Whether `device` is on, from its shadow.
pub async fn power_status(ctx: &Ctx<'_>, device: &Listed) -> Result<Option<bool>, CliError> {
    let mut tapo = connect(ctx).await?;
    Ok(read_one(&mut tapo, device.info.id()).await?.is_on())
}

/// How long a change waits for the device to report it.
const READ_BACK_TRIES: u32 = 8;
const READ_BACK_PAUSE: std::time::Duration = std::time::Duration::from_millis(750);

/// Ask `device` to turn on or off, building on its current shadow version,
/// then read the shadow back: the write's answer proves nothing, so the
/// state returned is the one the device reports (`None` if it never does).
pub async fn set_power(ctx: &Ctx<'_>, device: &Listed, on: bool) -> Result<Option<bool>, CliError> {
    let mut tapo = connect(ctx).await?;
    let id = device.info.id().to_string();
    let current = read_one(&mut tapo, &id).await?;
    let version = current.version() + 1;
    tapo.call(|c| {
        let id = id.clone();
        async move { c.set_desired(&id, json!({"on": on}), version).await }
    })
    .await?;
    let mut last = None;
    for _ in 0..READ_BACK_TRIES {
        tokio::time::sleep(READ_BACK_PAUSE).await;
        last = read_one(&mut tapo, &id).await?.is_on();
        if last == Some(on) {
            break;
        }
    }
    match last {
        Some(state) if state == on => Ok(Some(state)),
        _ => Err(CliError::Upstream(format!(
            "asked `{}` to turn {}, but it has not reported the change",
            device.name(),
            if on { "on" } else { "off" }
        ))),
    }
}

/// What `devices get` shows for a shadow device: its reported state.
pub async fn sys_info(ctx: &Ctx<'_>, device: &Listed) -> Result<Option<Value>, CliError> {
    let mut tapo = connect(ctx).await?;
    Ok(read_one(&mut tapo, device.info.id())
        .await?
        .reported()
        .cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_reads_booleans_and_numbers() {
        let s = Shadow {
            raw: json!({"thingName": "a", "version": 7, "state": {"reported": {"on": true}}}),
        };
        assert_eq!(s.is_on(), Some(true));
        assert_eq!(s.version(), 7);
        assert_eq!(on_of(&json!({"on": 0})), Some(false));
        assert_eq!(on_of(&json!({"on": 1})), Some(true));
        assert_eq!(on_of(&json!({})), None);
        let empty = Shadow { raw: json!({}) };
        assert_eq!(empty.is_on(), None);
        assert_eq!(empty.version(), 0);
    }
}
