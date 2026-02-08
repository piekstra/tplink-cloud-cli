# tplc - TP-Link Cloud CLI

Rust CLI for controlling TP-Link Kasa and Tapo smart home devices via the cloud API. Companion to the [tplink-cloud-api](https://github.com/piekstra/tplink-cloud-api) Python library.

## Quick start

```bash
cargo build                           # build
cargo fmt --check                     # check formatting
cargo clippy -- -D warnings           # lint
cargo test                            # run tests
```

## Architecture

### Dual-cloud support (Kasa + Tapo)

Both Kasa and Tapo use the same TP-Link V2 Cloud API with HMAC-SHA1 signing, but differ in:

| Aspect | Kasa | Tapo |
|--------|------|------|
| Host | `n-wap.tplinkcloud.com` | `n-wap.i.tplinkcloud.com` |
| Signing keys | Different per cloud (app-level, from APK) | Different per cloud (app-level, from APK) |
| Passthrough | `POST /` with `{"method":"passthrough","params":{...}}` | `POST /api/v2/common/passthrough` with flat body |

`CloudType` enum in `src/api/cloud_type.rs` centralizes all per-cloud configuration.

### Key files

| File | Purpose |
|------|---------|
| `src/api/cloud_type.rs` | `CloudType` enum with per-cloud host, keys, app type, passthrough format |
| `src/api/signing.rs` | HMAC-SHA1 request signing (ported from Python lib's `signing.py`) |
| `src/api/client.rs` | `TPLinkApi` — auth operations (login, MFA, token refresh, device list) |
| `src/api/device_client.rs` | `DeviceClient` — passthrough commands to individual devices |
| `src/models/device.rs` | `Device` struct with all operations (power, energy, light, schedule) |
| `src/models/device_type.rs` | `DeviceType` enum with capability checks (`has_emeter`, `is_light`, etc.) |
| `src/resolve.rs` | Device resolution across both clouds with deduplication |
| `src/auth/credentials.rs` | `AuthContext` with dual-cloud token management |
| `src/auth/keychain.rs` | OS keychain storage via `keyring` crate |
| `src/cli/mod.rs` | Full command tree (clap derive) |

### Signing algorithm

1. `content_md5 = base64(md5(body_json))`
2. `nonce = uuid4()`
3. `sig = hmac_sha1(cloud.secret_key, "{content_md5}\n9999999999\n{nonce}\n{url_path}").hex()`
4. Header: `X-Authorization: Timestamp=9999999999, Nonce={nonce}, AccessKey={cloud.access_key}, Signature={sig}`

The access/secret keys are app-level constants extracted from the Android APKs. They identify the app, not the user.

### Device passthrough

All device commands use a passthrough pattern — the cloud API forwards JSON commands to the device. Response data is double-JSON-encoded (a JSON string inside JSON). `DeviceClient::passthrough()` handles the encoding/decoding.

Child devices (multi-outlet strips like HS300, KP303) inject `"context": {"child_ids": ["child_id"]}` into the request data.

### Auth flow

1. Login to Kasa cloud (required) and Tapo cloud (best-effort, non-fatal if it fails)
2. Store separate tokens for each cloud in OS keychain
3. Auto-refresh on token expiry (error code -20651)
4. Credential sources: env vars (`TPLC_USERNAME`/`TPLC_PASSWORD`) -> keychain -> interactive prompt

### Error handling

Exit codes: 0=success, 1=general, 2=auth, 3=device_not_found, 4=device_offline. Errors output structured JSON to stderr.

## Using tplc as a Claude Code skill/plugin

`tplc` is designed for AI agent use. All commands output machine-parseable JSON by default. Here's how to integrate it:

### Skill definition

```markdown
# Smart Home Control (tplc)

Control TP-Link Kasa and Tapo smart home devices.

## Prerequisites
- `tplc` binary installed and on PATH
- Authenticated: run `tplc login` first (interactive, one-time setup)

## Available commands

### List devices
`tplc devices list`
Returns JSON array of all devices with alias, model, cloud type, status, device_id.

### Power control
`tplc power on|off|toggle|status "<device name>"`
Device name supports exact match, case-insensitive match, or partial match.

### Energy monitoring (HS110, KP115, KP125, P110, HS300 outlets only)
`tplc energy realtime "<device>"`
Returns voltage_mv, current_ma, power_mw, total_wh.

### Light control (KL430, KL420L5, L530 only)
`tplc light brightness "<device>" <0-100>`
`tplc light color "<device>" --hue <0-360> --saturation <0-100>`
`tplc light temp "<device>" <2500-9000>`
`tplc light state "<device>"`

### Device info
`tplc info sysinfo "<device>"`
`tplc power status "<device>"`

### Schedules
`tplc schedule list "<device>"`
`tplc schedule add "<device>" --action on --time 07:00 --days mon,tue,wed`

## Output format
- stdout: JSON (machine-readable)
- stderr: JSON error objects with `error`, `message`, `error_code` fields
- Exit codes: 0=success, 1=general, 2=auth, 3=device_not_found, 4=device_offline

## Error handling
If exit code is 2 (auth error), suggest the user run `tplc login`.
If exit code is 3 (device not found), try `tplc devices list` to show available devices.
```

### Example agent workflow

```bash
# 1. Check what devices are available
tplc devices list

# 2. Turn on a device (partial name match works)
tplc power on "living room"

# 3. Check energy usage
tplc energy realtime "Kitchen Plug"

# 4. Set a schedule
tplc schedule add "Porch Light" --action on --sunset --days mon,tue,wed,thu,fri,sat,sun
```

### Tips for agent integration

- Always check exit codes. Non-zero means the stdout JSON should be ignored.
- Device names are flexible: exact alias > device ID > case-insensitive > partial match.
- Use `--verbose` / `-v` flag when debugging API issues (logs HTTP requests to stderr).
- Use `--table` / `-t` flag when showing results to humans.
- The `tplc devices list` output includes a `cloud` field ("kasa" or "tapo") for each device.

## Reference

- [tplink-cloud-api](https://github.com/piekstra/tplink-cloud-api) - Python library this CLI ports
