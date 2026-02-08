# tplc - TP-Link Cloud CLI

A cross-platform CLI for controlling TP-Link Kasa smart home devices via the cloud API. Built in Rust for speed and portability.

## Installation

### Pre-built binaries

Download the latest release for your platform from [GitHub Releases](https://github.com/piekstra/tplink-cloud-cli/releases).

### From source

```bash
cargo install --git https://github.com/piekstra/tplink-cloud-cli
```

## Quick start

```bash
# Authenticate (interactive prompt with MFA support)
tplc login

# List all devices
tplc devices list

# Turn a device on/off
tplc power on "Living Room Lamp"
tplc power toggle "Porch Light"

# Check power status
tplc power status "Living Room Lamp"
```

## Commands

### Authentication

```bash
tplc login              # Interactive login (supports MFA)
tplc logout             # Clear stored credentials
tplc status             # Check authentication status
```

Credentials can also be provided via environment variables:
- `TPLC_USERNAME` - TP-Link/Kasa account email
- `TPLC_PASSWORD` - Account password

Tokens are stored securely in your OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service).

### Devices

```bash
tplc devices list                   # List all devices
tplc devices get "Device Name"      # Get device details
tplc devices search "lamp"          # Search by partial name
```

### Power control

```bash
tplc power on "Device Name"         # Turn on
tplc power off "Device Name"        # Turn off
tplc power toggle "Device Name"     # Toggle state
tplc power status "Device Name"     # Check on/off
```

### Energy monitoring

For devices with energy monitoring (HS110, KP115, KP125, HS300 outlets):

```bash
tplc energy realtime "Device Name"              # Current power draw
tplc energy daily "Device Name"                 # Daily stats (current month)
tplc energy daily "Device Name" --year 2026 --month 1
tplc energy monthly "Device Name"               # Monthly stats (current year)
tplc energy summary                             # All emeter devices
```

### Light strip controls

For light strip devices (KL430, KL420L5):

```bash
tplc light brightness "Strip" 75                        # Set brightness (0-100)
tplc light color "Strip" --hue 240 --saturation 100     # Set color
tplc light temp "Strip" 4000                            # Color temperature (2500-9000K)
tplc light state "Strip"                                # Get current state
```

### Schedules

```bash
tplc schedule list "Device Name"
tplc schedule add "Device Name" --action on --time 07:00 --days mon,tue,wed,thu,fri
tplc schedule add "Device Name" --action off --sunset
tplc schedule edit "Device Name" RULE_ID --disable
tplc schedule delete "Device Name" RULE_ID
tplc schedule clear "Device Name"               # Delete all rules
```

### Device info

```bash
tplc info sysinfo "Device Name"     # System information
tplc info network "Device Name"     # WiFi info (SSID, signal)
tplc info time "Device Name"        # Device clock
tplc led on "Device Name"           # Turn indicator LED on
tplc led off "Device Name"          # Turn indicator LED off
```

## Output format

Default output is JSON (machine-readable). Add `--table` or `-t` for human-readable tables:

```bash
tplc devices list -t
```

```
╭──────────────────┬────────┬────────┬────────┬────────┬───────────────╮
│ NAME             │ MODEL  │ TYPE   │ STATUS │ EMETER │ DEVICE ID     │
├──────────────────┼────────┼────────┼────────┼────────┼───────────────┤
│ Living Room Lamp │ KP115  │ plug   │ online │ yes    │ 80067B24...   │
│ Porch Light      │ HS200  │ switch │ online │ no     │ A3F19C02...   │
╰──────────────────┴────────┴────────┴────────┴────────┴───────────────╯
```

Errors are output as JSON to stderr with appropriate exit codes:

| Exit code | Meaning |
|-----------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Authentication error |
| 3 | Device not found |
| 4 | Device offline |

## Device resolution

Devices can be referenced by:
1. Exact alias (case-sensitive)
2. Device ID
3. Case-insensitive alias match
4. Partial alias match (if unambiguous)

Multi-outlet devices (HS300, KP303, KP400, etc.) expose each outlet as a separate device addressable by its alias.

## Supported devices

| Model | Type | Energy monitoring |
|-------|------|:-:|
| HS100, HS103, HS105 | Smart Plug | |
| HS110 | Smart Plug | Yes |
| HS200 | Smart Switch | |
| HS300 | Smart Power Strip (6 outlets) | Yes (per outlet) |
| KP115, KP125 | Smart Plug | Yes |
| KP200, KP400 | Outdoor Plug (2 outlets) | |
| KP303 | Smart Power Strip (3 outlets) | |
| EP40 | Outdoor Plug | |
| KL420L5, KL430 | Smart Light Strip | |

## Related

- [tplink-cloud-api](https://github.com/piekstra/tplink-cloud-api) - Python library this CLI is based on

## License

MIT
