use std::collections::HashSet;

use crate::api::client::TPLinkApi;
use crate::api::cloud_type::CloudType;
use crate::api::device_client::DeviceClient;
use crate::auth::credentials::{get_auth_context, refresh_auth, refresh_tapo_auth, AuthContext};
use crate::error::AppError;
use crate::models::device::Device;
use crate::models::device_info::DeviceInfo;
use crate::models::device_type::DeviceType;

/// Fetch all devices (including children) from both Kasa and Tapo clouds.
/// Deduplicates devices that appear in both clouds (Kasa takes priority).
pub async fn fetch_all_devices(
    verbose: bool,
) -> Result<(Vec<(DeviceInfo, DeviceType, Option<String>)>, AuthContext), AppError> {
    let mut auth = get_auth_context(verbose).await?;

    // Fetch Kasa devices
    let kasa_devices = fetch_devices_for_cloud(&mut auth, CloudType::Kasa, verbose).await?;

    // Track Kasa device IDs for deduplication
    let kasa_ids: HashSet<String> = kasa_devices
        .iter()
        .map(|(info, _, _)| info.id().to_string())
        .collect();

    let mut devices = kasa_devices;

    // Fetch Tapo devices (best-effort)
    if auth.has_tapo() {
        match fetch_devices_for_cloud(&mut auth, CloudType::Tapo, verbose).await {
            Ok(tapo_devices) => {
                for device in tapo_devices {
                    // Deduplicate: skip if already in Kasa
                    if !kasa_ids.contains(device.0.id()) {
                        devices.push(device);
                    }
                }
            }
            Err(e) => {
                if verbose {
                    eprintln!("Tapo device fetch failed (non-fatal): {}", e);
                }
            }
        }
    }

    Ok((devices, auth))
}

/// Fetch devices from a single cloud.
async fn fetch_devices_for_cloud(
    auth: &mut AuthContext,
    cloud_type: CloudType,
    verbose: bool,
) -> Result<Vec<(DeviceInfo, DeviceType, Option<String>)>, AppError> {
    let (token, regional_url) = match cloud_type {
        CloudType::Kasa => (auth.token.clone(), auth.regional_url.clone()),
        CloudType::Tapo => {
            let token = auth
                .tapo_token
                .as_ref()
                .ok_or(AppError::NotAuthenticated)?
                .clone();
            let url = auth
                .tapo_regional_url
                .as_ref()
                .ok_or(AppError::NotAuthenticated)?
                .clone();
            (token, url)
        }
    };

    let api = TPLinkApi::new(
        Some(regional_url),
        verbose,
        Some(auth.term_id.clone()),
        cloud_type,
    )?;

    let device_list = match api.get_device_info_list(&token).await {
        Ok(list) => list,
        Err(AppError::TokenExpired { .. }) => {
            match cloud_type {
                CloudType::Kasa => refresh_auth(auth, verbose).await?,
                CloudType::Tapo => refresh_tapo_auth(auth, verbose).await?,
            }
            let refreshed_token = match cloud_type {
                CloudType::Kasa => auth.token.clone(),
                CloudType::Tapo => auth
                    .tapo_token
                    .as_ref()
                    .ok_or(AppError::NotAuthenticated)?
                    .clone(),
            };
            api.get_device_info_list(&refreshed_token).await?
        }
        Err(e) => return Err(e),
    };

    let mut devices = Vec::new();

    for device_json in &device_list {
        if let Some(mut info) = DeviceInfo::from_json(device_json) {
            info.cloud_type = Some(cloud_type);
            let dtype = DeviceType::from_model(info.model());

            if dtype.has_children() {
                let client = DeviceClient::new(
                    info.app_server_url.as_deref().unwrap_or(&api.host),
                    &token,
                    &auth.term_id,
                    verbose,
                    cloud_type,
                )?;

                let parent_device =
                    Device::new(client, info.id().to_string(), info.clone(), dtype, None);

                // Add parent
                devices.push((info.clone(), dtype, None));

                // Add children
                if let Ok(children) = parent_device.get_children().await {
                    for child in children {
                        let child_alias = if child.alias.is_empty() {
                            None
                        } else {
                            Some(child.alias)
                        };
                        devices.push((info.clone(), dtype.child_type(), child_alias));
                    }
                }
            } else {
                devices.push((info, dtype, None));
            }
        }
    }

    Ok(devices)
}

/// Resolve a device by name or ID, searching both Kasa and Tapo clouds.
pub async fn resolve_device(name_or_id: &str, verbose: bool) -> Result<Device, AppError> {
    let mut auth = get_auth_context(verbose).await?;

    // Build flat list from both clouds
    let mut all_devices: Vec<(DeviceInfo, DeviceType, Option<String>, Option<String>)> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    // Kasa devices
    collect_devices_for_resolution(
        &mut auth,
        CloudType::Kasa,
        verbose,
        &mut all_devices,
        &mut seen_ids,
    )
    .await?;

    // Tapo devices (best-effort)
    if auth.has_tapo() {
        if let Err(e) = collect_devices_for_resolution(
            &mut auth,
            CloudType::Tapo,
            verbose,
            &mut all_devices,
            &mut seen_ids,
        )
        .await
        {
            if verbose {
                eprintln!("Tapo device fetch failed (non-fatal): {}", e);
            }
        }
    }

    // Resolution priority:
    // 1. Exact alias match
    // 2. Exact device_id match
    // 3. Case-insensitive alias match
    // 4. Partial alias match (only if exactly one result)

    let name_lower = name_or_id.to_lowercase();

    // 1. Exact alias match
    for (info, dtype, child_alias, child_id) in &all_devices {
        let alias = child_alias.as_deref().unwrap_or(info.alias_or_name());
        if alias == name_or_id {
            return build_device(info, *dtype, child_id.clone(), &auth, verbose);
        }
    }

    // 2. Exact device_id match
    for (info, dtype, _, child_id) in &all_devices {
        if info.id() == name_or_id {
            return build_device(info, *dtype, child_id.clone(), &auth, verbose);
        }
    }

    // 3. Case-insensitive alias match
    for (info, dtype, child_alias, child_id) in &all_devices {
        let alias = child_alias.as_deref().unwrap_or(info.alias_or_name());
        if alias.to_lowercase() == name_lower {
            return build_device(info, *dtype, child_id.clone(), &auth, verbose);
        }
    }

    // 4. Partial alias match
    let partial_matches: Vec<_> = all_devices
        .iter()
        .filter(|(info, _, child_alias, _)| {
            let alias = child_alias.as_deref().unwrap_or(info.alias_or_name());
            alias.to_lowercase().contains(&name_lower)
        })
        .collect();

    if partial_matches.len() == 1 {
        let (info, dtype, _, child_id) = partial_matches[0];
        return build_device(info, *dtype, child_id.clone(), &auth, verbose);
    }

    if partial_matches.len() > 1 {
        let names: Vec<String> = partial_matches
            .iter()
            .map(|(info, _, child_alias, _)| {
                child_alias
                    .as_deref()
                    .unwrap_or(info.alias_or_name())
                    .to_string()
            })
            .collect();
        return Err(AppError::DeviceNotFound(format!(
            "Multiple devices match '{}': {}",
            name_or_id,
            names.join(", ")
        )));
    }

    Err(AppError::DeviceNotFound(name_or_id.to_string()))
}

/// Collect devices from one cloud into the all_devices list for resolution.
async fn collect_devices_for_resolution(
    auth: &mut AuthContext,
    cloud_type: CloudType,
    verbose: bool,
    all_devices: &mut Vec<(DeviceInfo, DeviceType, Option<String>, Option<String>)>,
    seen_ids: &mut HashSet<String>,
) -> Result<(), AppError> {
    let (token, regional_url) = match cloud_type {
        CloudType::Kasa => (auth.token.clone(), auth.regional_url.clone()),
        CloudType::Tapo => {
            let token = auth
                .tapo_token
                .as_ref()
                .ok_or(AppError::NotAuthenticated)?
                .clone();
            let url = auth
                .tapo_regional_url
                .as_ref()
                .ok_or(AppError::NotAuthenticated)?
                .clone();
            (token, url)
        }
    };

    let api = TPLinkApi::new(
        Some(regional_url),
        verbose,
        Some(auth.term_id.clone()),
        cloud_type,
    )?;

    let device_list = match api.get_device_info_list(&token).await {
        Ok(list) => list,
        Err(AppError::TokenExpired { .. }) => {
            match cloud_type {
                CloudType::Kasa => refresh_auth(auth, verbose).await?,
                CloudType::Tapo => refresh_tapo_auth(auth, verbose).await?,
            }
            let refreshed_token = match cloud_type {
                CloudType::Kasa => auth.token.clone(),
                CloudType::Tapo => auth
                    .tapo_token
                    .as_ref()
                    .ok_or(AppError::NotAuthenticated)?
                    .clone(),
            };
            api.get_device_info_list(&refreshed_token).await?
        }
        Err(e) => return Err(e),
    };

    for device_json in &device_list {
        if let Some(mut info) = DeviceInfo::from_json(device_json) {
            // Deduplicate: Kasa takes priority
            if !seen_ids.insert(info.id().to_string()) {
                continue;
            }

            info.cloud_type = Some(cloud_type);
            let dtype = DeviceType::from_model(info.model());

            if dtype.has_children() {
                let client = DeviceClient::new(
                    info.app_server_url.as_deref().unwrap_or(&api.host),
                    &token,
                    &auth.term_id,
                    verbose,
                    cloud_type,
                )?;

                let parent_device =
                    Device::new(client, info.id().to_string(), info.clone(), dtype, None);

                // Add parent (no child_id)
                all_devices.push((info.clone(), dtype, None, None));

                if let Ok(children) = parent_device.get_children().await {
                    for child in children {
                        let child_alias = if child.alias.is_empty() {
                            None
                        } else {
                            Some(child.alias)
                        };
                        all_devices.push((
                            info.clone(),
                            dtype.child_type(),
                            child_alias,
                            Some(child.id),
                        ));
                    }
                }
            } else {
                all_devices.push((info, dtype, None, None));
            }
        }
    }

    Ok(())
}

fn build_device(
    info: &DeviceInfo,
    dtype: DeviceType,
    child_id: Option<String>,
    auth: &AuthContext,
    verbose: bool,
) -> Result<Device, AppError> {
    let cloud_type = info.cloud_type.unwrap_or(CloudType::Kasa);

    let (token, regional_url) = match cloud_type {
        CloudType::Kasa => (auth.token.clone(), auth.regional_url.clone()),
        CloudType::Tapo => {
            let token = auth
                .tapo_token
                .as_ref()
                .ok_or(AppError::NotAuthenticated)?
                .clone();
            let url = auth
                .tapo_regional_url
                .as_ref()
                .ok_or(AppError::NotAuthenticated)?
                .clone();
            (token, url)
        }
    };

    let client = DeviceClient::new(
        info.app_server_url.as_deref().unwrap_or(&regional_url),
        &token,
        &auth.term_id,
        verbose,
        cloud_type,
    )?;

    Ok(Device::new(
        client,
        info.id().to_string(),
        info.clone(),
        dtype,
        child_id,
    ))
}
