# Contributing to tplc

Thanks for your interest in contributing! This document outlines how to get started.

## Development setup

1. Clone the repository:
   ```bash
   git clone https://github.com/piekstra/tplink-cloud-cli.git
   cd tplink-cloud-cli
   ```

2. Build:
   ```bash
   cargo build
   ```

3. Run checks:
   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test
   ```

## Project structure

```
src/
  main.rs                    # Entry point
  lib.rs                     # Command dispatch
  error.rs                   # AppError enum with exit codes
  config.rs                  # RuntimeConfig (output mode, verbose)
  resolve.rs                 # Device resolution (both clouds)
  api/
    cloud_type.rs            # CloudType enum (Kasa/Tapo)
    signing.rs               # HMAC-SHA1 request signing
    client.rs                # Auth HTTP client (login, MFA, refresh)
    device_client.rs         # Device passthrough requests
    response.rs / errors.rs  # API response parsing
  models/
    device.rs                # Device struct with all operations
    device_type.rs           # DeviceType enum + capabilities
    device_info.rs           # Device metadata from API
    energy.rs / schedule.rs  # Domain models
  auth/
    keychain.rs              # OS keychain storage
    credentials.rs           # Auth context management
    token.rs                 # TokenSet struct
  cli/
    mod.rs                   # CLI command tree (clap)
    auth.rs / devices.rs     # Command handlers
    output.rs                # JSON + table rendering
```

## Submitting changes

1. Fork the repository
2. Create a feature branch (`git checkout -b my-feature`)
3. Make your changes
4. Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test`
5. Commit your changes with a descriptive message
6. Push to your fork and open a pull request

## Adding support for new devices

If you'd like to add support for a new TP-Link device:

1. Open an issue first to discuss the device and its capabilities
2. Add the model prefix to `DeviceType` in `src/models/device_type.rs`
3. Update capability methods (`has_emeter()`, `is_light()`, etc.) if needed
4. Add the display name in `display_name()`
5. Add unit tests for the new device type
6. Update the README with the new device in the compatibility list

## Adding support for a new cloud ecosystem

The dual-cloud architecture (Kasa + Tapo) is designed to be extensible. To add a new cloud:

1. Add a variant to `CloudType` in `src/api/cloud_type.rs`
2. Fill in the cloud's host, keys, app type, and passthrough format
3. Update `DeviceClient::passthrough()` if the new cloud uses a different protocol
4. Update the auth flow in `src/cli/auth.rs` and token storage

## Questions?

Feel free to open an issue if you have questions or need help getting started.
