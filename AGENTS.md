# AGENTS.md

Guidance for AI coding agents (and humans) working in this repo. Tool-agnostic;
`CLAUDE.md` points here.

## What this is

`tplc` — a Rust CLI over the TP-Link cloud APIs behind the Kasa and Tapo
apps: the signed v2 account cloud (login, device list, device passthrough),
the Kasa IoT cloud (device groups), and the Tapo NBU app-server (homes and
rooms). A thin, TP-Link-specific layer over the shared
[`cli-common`](https://github.com/piekstra/cli-common) `pk-cli-*` crates
(**v0.29.0**: output, errors, confirm gate, reference ladder, keychain items,
config, self-update, auth shapes, refresh lock, mcp). This repo owns only the vendor clients,
the device models, and the commands.

## Build, test, lint

```console
make verify     # fmt-check + clippy -D warnings + tests + smoke — the CI gate
make test
make install    # cargo install + re-sign so keychain grants survive
```

Run `make verify` before considering a change done — it's exactly what CI runs.

## Layout

- `src/lib.rs` — `run`: offline commands first, then arg validation, then
  the async runtime for everything that talks to a cloud; `info`.
- `src/cli/mod.rs` — the clap tree. `src/cli/<group>.rs` — one handler
  module per command group (`auth`, `config`, `devices`, `power`, `energy`,
  `light`, `schedule`, `info`, `led`, `rooms`, `groups`, `api`); `cli/emit.rs`
  holds `Ctx` and the headed-list renderer. `dispatch` in `lib.rs` only
  translates and delegates — no command logic lives there.
- `src/mcp.rs` — `tplc mcp`; see "The MCP tools".
- `src/session.rs` — the keychain layout (one `session` item + a parked
  `mfa_pending` item under `piekstra.tplc`), legacy migrations, token
  refresh under the shared lock, and `with_refresh` (the one "call; on an expired token refresh
  once and retry" policy every cloud call goes through). `Sessions` is
  generic over a `SecretItems` seam so the migration order is unit-tested
  against an in-memory store.
- `src/api/client.rs` — the v2 account cloud: signing, regional URL, login,
  MFA, refresh, `getDeviceList`, method passthrough, `getAppServiceUrl`.
- `src/api/device_client.rs` — device passthrough (Kasa vs Tapo envelopes).
- `src/api/nbu.rs` — the Tapo NBU app-server (families, rooms, things).
- `src/api/signing.rs`, `cloud_type.rs`, `errors.rs`, `response.rs`.
- `src/models/` — `Device` operations, `DeviceType` capabilities, typed
  passthrough payloads. `src/resolve.rs` — discovery across both clouds.
- `src/error.rs` — `AppError` (vendor) → `CliError` (exit codes), one place.
- `tests/` — offline surface tests + fixture contract tests; see
  `tests/fixtures/README.md`.
- `docs/api.md` — every endpoint, envelope, header set, and trap known so far.

## Conventions (do not break these)

- **`--json` on every command**, one DTO tagged `"schema": "<name>/v1"`, one
  document per invocation. Text (default) → stdout via the shared renderer;
  diagnostics → stderr. Keep both paths in sync.
- **Exit codes:** 0 ok · 2 usage · 3 auth · 4 not found · 5 upstream · 6
  confirmation required. The `AppError → CliError` table in `src/error.rs`
  is the only mapping. Validate args **before** touching the keychain or
  network, so `--help` and bad input never prompt or hang.
- **Secrets** come from the keychain (`piekstra.tplc`), `--stdin`,
  `--from-env`, or `$TPLC_PASSWORD` — never argv, never logs, never a file.
  `-v` prints request bodies only through `api::client::redacted`, which
  blanks `cloudPassword`, tokens and MFA codes; keep new verbose lines
  behind it.
  The password is used once and dropped; only tokens are stored, as **one**
  keychain item (every extra item is a macOS prompt per rebuild).
- **Room writes gate first, read back after.** `rooms move|create|rename|
  delete` call `pk_cli_core::confirm::require_confirmable` before any I/O
  (exit 6 non-interactively without `--force`), prompt with resolved names,
  and verify with a fresh `GET` before emitting success — the NBU write
  endpoints answer with empty bodies, so a 2xx proves nothing.
- **References resolve through `pk_cli_core::resolve::pick`** (exact name →
  exact id → case-insensitive name → unique partial); ambiguity is an error
  naming candidates, never a silent first pick. Devices, rooms, homes,
  things all use it.
- **`device-rooms/v1` is a contract** (cli-common DESIGN.md §1.8): `id`,
  `name`, `room`, `source` — omit `name` when unknown and `room` when the
  vendor app files the device in no room; never emit null. The consumer
  (`ghome audit --expect -`) reports a roomless row as `unfiled`.
- `api` is an RPC-by-method passthrough, deliberately not the family's HTTP
  `api <VERB> <PATH>` form (there are no paths to expose).

## The MCP tools

`src/mcp.rs` is `tplc mcp` (cli-common `DESIGN.md` §1.9). Three tools read; `tplink_power` and
`tplink_light` change one device, follow my-q-cli's pattern, and are not read-only. A write names one
device by its full name or id (`named_device` leaves the resolver's partial match out), is refused for
a strip, a model `DeviceType` does not know and an offline device (`check_may_change`), can send only a
`Change`, and returns the state read back with `confirmed`, also when the command's own answer was lost
(`Sent::NoAnswer`). A new argument or tool goes on the lists in `src/mcp/tests.rs`, and a new tool in
`packaging/mcpb/manifest.json` and README's table. The definitions stay under 8,000 characters together.
No tool removes or unpairs a device, changes an account, a schedule, a room, a group, the LED or
firmware, shares access, or reaches `tplc api`, whatever the CLI can do. A `get_sysinfo` answer carries
the device's MAC address and coordinates: `read_state` copies named fields only, and must go on doing so.
A write by name is refused while `Fetched.gaps` is non-empty. A strip the cloud lists as offline is in `Fetched.unseen`: the MCP path does not ask it for its outlets, says so in `note`, and goes on, accepting that a name shared with an outlet on it reaches the online device. No command goes out 75 s after the call began (`SEND_DEADLINE`), which also covers a keychain prompt answered late. `resolve::Patience` keeps a whole call
inside the 120 s `--http` gives it. Every write of the session item outside a sign-in goes through
`RefreshLock`; a copy read earlier can carry a retired refresh token, so `Sessions::store` is not for
a cache (see `remember_app_server`).

## Releasing

Bump `version` in `Cargo.toml` and `packaging/mcpb/manifest.json`, add to `CHANGELOG.md`, merge, then push
the tag `v<version>`. The workflow checks the tag, crate and manifest agree, builds the tarballs
`self-update` finds and the `.mcpb`, publishes once all of them are there, and opens the Homebrew
formula's pull request. A pull request that touches the workflow, packaging or dependencies runs
everything but the publish as a dry run.

## Tests

Offline, always. No test may read the OS keychain: `cargo test` produces an
ad-hoc-signed binary that macOS treats as a new identity, so a credentialed
command would prompt — and on a machine with a legacy `tplc` session, would
*migrate* it from a test binary. The session lives only in the keychain
(there is no config-side identity gate), so the two assertions that need it
("`auth status` works logged out", "credentialed reads exit 3") run only with
`TPLC_TEST_KEYCHAIN=1`, on a machine whose keychain holds no `tplc` session.
Everything else — `--help` everywhere, `info`, usage errors, the exit-6
gate, login input validation, completions, the PII scan — runs by default.
The real binary's `tests/mcp_stdio.rs` and `tests/mcp_http.rs` stop at calls that fail on their
arguments for that reason, and `src/mcp/tests.rs` runs the live path against a stub of the clouds on
127.0.0.1 over an in-memory keychain and a lock in the temp directory; no test can reach a real device.
Live checks go through the installed binary by hand.

## Safety & privacy (public repo)

- Nothing tracked in git may carry a real email, device id, MAC, token, or
  coordinates. `tests/cli_surface.rs` scans `git ls-files` for those
  shapes; fixtures use zero-padded ids and `@example.com`.
- Runtime output legitimately carries all of that; that's the tool's job.
- Never run `auth login`, a `power`/`light`/`schedule` write, or a `rooms`
  write to "test" it: they act on the owner's real account and devices. The
  encoders are unit-tested and the wire shapes are in `docs/api.md`.

## Definition of done

`make verify` green, CI green, the change dogfooded through the installed
binary, `--json` and text output in sync, `docs/api.md` matching reality,
CHANGELOG entry for anything user-visible, and no secrets or personal data
anywhere in the diff.
