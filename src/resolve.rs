//! Device discovery across both clouds, and the name-or-id resolution ladder
//! every device command starts from.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use pk_cli_core::CliError;
use serde_json::{json, Value};

use crate::api::client::TPLinkApi;
use crate::api::cloud_type::CloudType;
use crate::api::device_client::{DeviceClient, PASSTHROUGH_TIMEOUT};
use crate::cli::emit::Ctx;
use crate::models::device::Device;
use crate::models::device_info::DeviceInfo;
use crate::models::device_type::DeviceType;
use crate::session::{self, SecretItems, Sessions, TokenSet};

/// One addressable device: a cloud-listed device, or one outlet of a
/// multi-outlet strip (then `child_id`/`child_alias` are set and `info` is
/// the parent's).
#[derive(Debug, Clone)]
pub struct Listed {
    pub info: DeviceInfo,
    pub dtype: DeviceType,
    pub child_id: Option<String>,
    pub child_alias: Option<String>,
}

impl Listed {
    /// The name a user addresses this device by.
    pub fn name(&self) -> &str {
        self.child_alias
            .as_deref()
            .unwrap_or(self.info.alias_or_name())
    }

    pub fn cloud(&self) -> CloudType {
        self.info.cloud_type.unwrap_or(CloudType::Kasa)
    }

    pub fn is_online(&self) -> bool {
        self.info.status == Some(1)
    }

    /// The `device-list/v1` row.
    pub fn row(&self) -> Value {
        json!({
            "alias": self.name(),
            "model": self.info.model(),
            "device_type": format!("{:?}", self.dtype),
            "category": self.dtype.category(),
            "cloud": self.cloud().display_name(),
            "status": if self.is_online() { "online" } else { "offline" },
            "energy_monitoring": self.dtype.has_emeter(),
            "device_id": self.info.id(),
        })
    }
}

/// Every device (and outlet) on the account: Kasa first, then Tapo
/// best-effort, de-duplicated by device id (Kasa wins).
pub async fn fetch_all_devices(ctx: &Ctx<'_>) -> Result<(Vec<Listed>, TokenSet), CliError> {
    fetch_all_devices_with(ctx, true).await
}

/// As [`fetch_all_devices`]; with `expand_children` false the outlets of a
/// strip are not queried (one call per cloud, no per-device round-trips),
/// for callers that only need cloud-level ids and names.
pub async fn fetch_all_devices_with(
    ctx: &Ctx<'_>,
    expand_children: bool,
) -> Result<(Vec<Listed>, TokenSet), CliError> {
    let mut tokens = ctx.session()?;
    let fetched = fetch_devices(
        ctx.sessions,
        &mut tokens,
        expand_children,
        ctx.verbose,
        Patience::CLI,
    )
    .await?;
    if ctx.verbose {
        for gap in &fetched.gaps {
            eprintln!("{gap} (non-fatal)");
        }
    }
    Ok((fetched.devices, tokens))
}

/// How long discovery waits. A person at a terminal can wait out a device the cloud is trying to
/// wake. A tool has to answer inside its call's limit, while the answer can still say what
/// became of a command.
#[derive(Debug, Clone, Copy)]
pub struct Patience {
    /// Each request to a device.
    pub request: Duration,
    /// Once this has passed, no further strip's outlets are asked for.
    pub listing: Option<Duration>,
}

impl Patience {
    pub const CLI: Patience = Patience {
        request: PASSTHROUGH_TIMEOUT,
        listing: None,
    };
}

/// The devices both clouds list, and what could not be read of them.
pub struct Fetched {
    pub devices: Vec<Listed>,
    /// A cloud or a strip's outlets that are missing, in a sentence. A reader told "these are
    /// your devices" can be told this too.
    pub gaps: Vec<String>,
}

/// Every device (and outlet) `tokens` can see, refreshing them as needed. Kasa is required;
/// Tapo, and each strip's outlets, are best-effort.
pub async fn fetch_devices<S: SecretItems>(
    sessions: &Sessions<S>,
    tokens: &mut TokenSet,
    expand_children: bool,
    verbose: bool,
    patience: Patience,
) -> Result<Fetched, CliError> {
    let wait = Listing {
        expand_children,
        verbose,
        patience,
        started: Instant::now(),
    };
    let mut fetched = fetch_cloud(sessions, tokens, CloudType::Kasa, wait).await?;
    if tokens.has_tapo() {
        match fetch_cloud(sessions, tokens, CloudType::Tapo, wait).await {
            Ok(tapo) => {
                let seen: HashSet<String> = fetched
                    .devices
                    .iter()
                    .map(|d| d.info.id().to_string())
                    .collect();
                fetched.devices.extend(
                    tapo.devices
                        .into_iter()
                        .filter(|d| !seen.contains(d.info.id())),
                );
                fetched.gaps.extend(tapo.gaps);
            }
            Err(e) => fetched
                .gaps
                .push(format!("Tapo's device list could not be read: {e}")),
        }
    }
    Ok(fetched)
}

/// One discovery's settings, and when it began.
#[derive(Clone, Copy)]
struct Listing {
    expand_children: bool,
    verbose: bool,
    patience: Patience,
    started: Instant,
}

impl Listing {
    fn out_of_time(&self) -> bool {
        self.patience
            .listing
            .is_some_and(|limit| self.started.elapsed() >= limit)
    }
}

/// One cloud's device list, refreshing that cloud's token once if expired.
async fn fetch_cloud<S: SecretItems>(
    sessions: &Sessions<S>,
    tokens: &mut TokenSet,
    cloud: CloudType,
    wait: Listing,
) -> Result<Fetched, CliError> {
    let Listing {
        expand_children,
        verbose,
        patience,
        ..
    } = wait;
    let device_list = session::with_refresh(sessions, tokens, cloud, verbose, |t| {
        let access = t.cloud_access(cloud);
        let term_id = t.term_id.clone();
        async move {
            let (token, regional_url) = access?;
            let api = TPLinkApi::new(Some(regional_url), verbose, Some(term_id), cloud)?;
            api.get_device_info_list(&token).await
        }
    })
    .await?;
    let (token, regional_url) = tokens.cloud_access(cloud)?;

    let mut devices = Vec::new();
    let mut gaps = Vec::new();
    for device_json in &device_list {
        let Some(info) = DeviceInfo::from_cloud(device_json, cloud) else {
            continue;
        };
        let dtype = DeviceType::from_model(info.model());
        devices.push(Listed {
            info: info.clone(),
            dtype,
            child_id: None,
            child_alias: None,
        });
        if expand_children && dtype.has_children() {
            if wait.out_of_time() {
                gaps.push(format!(
                    "The outlets of `{}` were not read, for lack of time.",
                    info.alias_or_name()
                ));
                continue;
            }
            let client = DeviceClient::with_timeout(
                info.app_server_url.as_deref().unwrap_or(&regional_url),
                &token,
                &tokens.term_id,
                verbose,
                cloud,
                patience.request,
            )?;
            let parent = Device::new(client, info.id().to_string(), info.clone(), dtype, None);
            // A strip whose outlets can't be read still lists as itself.
            match parent.get_children().await {
                Ok(children) => {
                    for child in children {
                        devices.push(Listed {
                            info: info.clone(),
                            dtype: dtype.child_type(),
                            child_id: Some(child.id),
                            child_alias: Some(child.alias).filter(|a| !a.is_empty()),
                        });
                    }
                }
                Err(e) => gaps.push(format!(
                    "The outlets of `{}` could not be read: {e}",
                    info.alias_or_name()
                )),
            }
        }
    }
    Ok(Fetched { devices, gaps })
}

/// Resolve a device by name or id across both clouds. Ladder:
/// exact alias → exact device id → case-insensitive alias → unique partial
/// alias. `NotFound` (exit 4) otherwise, naming the candidates when the
/// partial match is ambiguous.
pub async fn resolve_device(ctx: &Ctx<'_>, name_or_id: &str) -> Result<Device, CliError> {
    let (devices, tokens) = fetch_all_devices(ctx).await?;
    let found = pick(&devices, name_or_id)?;
    build_device(found, &tokens, ctx.verbose)
}

/// The family ladder (`pk_cli_core::resolve::pick`): exact name → exact id →
/// case-insensitive name → unique partial name; ambiguity names the
/// candidates. An id names the strip itself, never one of its outlets (they
/// share it), so outlets answer to no id.
pub(crate) fn pick<'a>(devices: &'a [Listed], name_or_id: &str) -> Result<&'a Listed, CliError> {
    pk_cli_core::resolve::pick(
        devices,
        name_or_id,
        |d| {
            if d.child_id.is_some() {
                vec![]
            } else {
                vec![d.info.id().to_string()]
            }
        },
        |d| d.name(),
        "device",
    )
}

pub(crate) fn build_device(
    listed: &Listed,
    tokens: &TokenSet,
    verbose: bool,
) -> Result<Device, CliError> {
    build_device_waiting(listed, tokens, verbose, PASSTHROUGH_TIMEOUT)
}

/// As [`build_device`], waiting no longer than `timeout` for each request.
pub(crate) fn build_device_waiting(
    listed: &Listed,
    tokens: &TokenSet,
    verbose: bool,
    timeout: Duration,
) -> Result<Device, CliError> {
    let cloud = listed.cloud();
    let (token, regional_url) = tokens.cloud_access(cloud)?;
    let client = DeviceClient::with_timeout(
        listed
            .info
            .app_server_url
            .as_deref()
            .unwrap_or(&regional_url),
        &token,
        &tokens.term_id,
        verbose,
        cloud,
        timeout,
    )?;
    Ok(Device::new(
        client,
        listed.info.id().to_string(),
        listed.info.clone(),
        listed.dtype,
        listed.child_id.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(id: &str, alias: &str, model: &str, child: Option<(&str, &str)>) -> Listed {
        let info = DeviceInfo::from_json(&json!({
            "deviceId": id, "alias": alias, "deviceModel": model, "status": 1
        }))
        .unwrap();
        let dtype = DeviceType::from_model(model);
        match child {
            None => Listed {
                info,
                dtype,
                child_id: None,
                child_alias: None,
            },
            Some((cid, calias)) => Listed {
                info,
                dtype: dtype.child_type(),
                child_id: Some(cid.into()),
                child_alias: Some(calias.into()),
            },
        }
    }

    fn fleet() -> Vec<Listed> {
        vec![
            listed(
                "0000000000000000000000000000000000000001",
                "Porch Light",
                "HS200(US)",
                None,
            ),
            listed(
                "0000000000000000000000000000000000000002",
                "Desk Lamp",
                "HS103(US)",
                None,
            ),
            listed(
                "0000000000000000000000000000000000000003",
                "Power Strip",
                "HS300(US)",
                None,
            ),
            listed(
                "0000000000000000000000000000000000000003",
                "Power Strip",
                "HS300(US)",
                Some(("000000000000000000000000000000000000000300", "Monitor")),
            ),
            listed(
                "0000000000000000000000000000000000000004",
                "Desk Fan",
                "KP115(US)",
                None,
            ),
        ]
    }

    #[test]
    fn resolution_ladder_in_order() {
        let f = fleet();
        assert_eq!(
            pick(&f, "Porch Light").unwrap().info.id(),
            "0000000000000000000000000000000000000001"
        );
        assert_eq!(
            pick(&f, "0000000000000000000000000000000000000002")
                .unwrap()
                .name(),
            "Desk Lamp"
        );
        assert_eq!(pick(&f, "porch light").unwrap().name(), "Porch Light");
        assert_eq!(
            pick(&f, "monitor").unwrap().child_id.as_deref(),
            Some("000000000000000000000000000000000000000300")
        );
        assert_eq!(pick(&f, "fan").unwrap().name(), "Desk Fan");
    }

    #[test]
    fn an_id_resolves_the_parent_not_an_outlet() {
        let f = fleet();
        let d = pick(&f, "0000000000000000000000000000000000000003").unwrap();
        assert!(d.child_id.is_none());
    }

    #[test]
    fn ambiguous_and_missing_names_are_not_found() {
        let f = fleet();
        let err = pick(&f, "desk").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Desk Lamp") && msg.contains("Desk Fan"),
            "{msg}"
        );
        let err = pick(&f, "garage").unwrap_err();
        assert!(err.to_string().contains("no device matching"), "{err}");
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn row_shape_leads_with_the_identifying_fields() {
        let f = fleet();
        let row = f[4].row();
        let keys: Vec<&String> = row.as_object().unwrap().keys().collect();
        assert_eq!(keys[0], "alias");
        assert_eq!(row["energy_monitoring"], true);
        assert_eq!(row["category"], "plug");
        assert_eq!(row["cloud"], "kasa");
    }
}
