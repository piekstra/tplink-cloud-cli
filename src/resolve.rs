use crate::api::client::TPLinkApi;
use crate::api::device_client::DeviceClient;
use crate::auth::credentials::{get_auth_context, refresh_auth, AuthContext};
use crate::error::AppError;
use crate::models::device::Device;
use crate::models::device_info::DeviceInfo;
use crate::models::device_type::DeviceType;

/// Fetch all devices (including children) and return them with their metadata.
pub async fn fetch_all_devices(
    verbose: bool,
) -> Result<(Vec<(DeviceInfo, DeviceType, Option<String>)>, AuthContext), AppError> {
    let mut auth = get_auth_context(verbose).await?;

    let api = TPLinkApi::new(
        Some(auth.regional_url.clone()),
        verbose,
        Some(auth.term_id.clone()),
    )?;

    let device_list = match api.get_device_info_list(&auth.token).await {
        Ok(list) => list,
        Err(AppError::TokenExpired { .. }) => {
            refresh_auth(&mut auth, verbose).await?;
            api.get_device_info_list(&auth.token).await?
        }
        Err(e) => return Err(e),
    };

    let mut devices = Vec::new();

    for device_json in &device_list {
        if let Some(info) = DeviceInfo::from_json(device_json) {
            let dtype = DeviceType::from_model(info.model());

            if dtype.has_children() {
                // Fetch children by getting sys_info
                let client = DeviceClient::new(
                    info.app_server_url.as_deref().unwrap_or(&auth.regional_url),
                    &auth.token,
                    &auth.term_id,
                    verbose,
                )?;

                let parent_device =
                    Device::new(client, info.id().to_string(), info.clone(), dtype, None);

                // Add parent
                devices.push((info.clone(), dtype, None));

                // Add children
                if let Ok(children) = parent_device.get_children().await {
                    for child in children {
                        // Use child alias if available
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

    Ok((devices, auth))
}

/// Resolve a device by name or ID, with auto token refresh.
pub async fn resolve_device(name_or_id: &str, verbose: bool) -> Result<Device, AppError> {
    let mut auth = get_auth_context(verbose).await?;

    let api = TPLinkApi::new(
        Some(auth.regional_url.clone()),
        verbose,
        Some(auth.term_id.clone()),
    )?;

    let device_list = match api.get_device_info_list(&auth.token).await {
        Ok(list) => list,
        Err(AppError::TokenExpired { .. }) => {
            refresh_auth(&mut auth, verbose).await?;
            api.get_device_info_list(&auth.token).await?
        }
        Err(e) => return Err(e),
    };

    // Build flat list of all devices including children
    let mut all_devices: Vec<(DeviceInfo, DeviceType, Option<String>, Option<String>)> = Vec::new();

    for device_json in &device_list {
        if let Some(info) = DeviceInfo::from_json(device_json) {
            let dtype = DeviceType::from_model(info.model());

            if dtype.has_children() {
                // Fetch children
                let client = DeviceClient::new(
                    info.app_server_url.as_deref().unwrap_or(&auth.regional_url),
                    &auth.token,
                    &auth.term_id,
                    verbose,
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

fn build_device(
    info: &DeviceInfo,
    dtype: DeviceType,
    child_id: Option<String>,
    auth: &AuthContext,
    verbose: bool,
) -> Result<Device, AppError> {
    let client = DeviceClient::new(
        info.app_server_url.as_deref().unwrap_or(&auth.regional_url),
        &auth.token,
        &auth.term_id,
        verbose,
    )?;

    Ok(Device::new(
        client,
        info.id().to_string(),
        info.clone(),
        dtype,
        child_id,
    ))
}
