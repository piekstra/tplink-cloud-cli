# Changelog

## 0.3.1 — 2026-11-03

- **Security fix:** a password or token now goes only to `https` on a TP-Link domain: `tplinkcloud.com`,
  `tplinknbu.com` or `tplinkra.com`. TP-Link's answers name the hosts that receive them (the account
  lookup, the service lookup and each device's app server), and a session stores those hosts. An
  address elsewhere is an upstream failure, exit 5, and nothing is sent to it.
- **Security fix:** no client follows a redirect off its host. The token is in the query and the password
  or refresh token in the body, which a 307 or 308 re-sends, and reqwest strips neither. A redirect to
  another host is now an error that says so, and a write refused this way is reported as a failure, not
  as an unknown outcome.
- A write's deadlines are now cli-common's `SendDeadline`, with the same 75 s to send and 90 s to start
  a read-back, on both clocks. Its refusal is an `other` error in place of `upstream`, since TP-Link did
  not fail, and it tells the caller to check the current state before calling again.
- cli-common v0.25.0, up from v0.23.0.

## 0.3.0 — 2026-11-03

- `tplc mcp`: tools for Claude. `tplink_devices`, `tplink_device_state` and `tplink_energy` read;
  `tplink_power` and `tplink_light` change one light, plug, wall switch or outlet, named in full, and
  a client that honors their annotations asks you before each call. Each returns the state read back
  and says whether it shows the change. They run over stdio for Claude Code and Claude Desktop, and
  with `--http` on 127.0.0.1 behind a bearer token for Claude on the web and mobile through your own
  Cloudflare Worker and tunnel, reads only unless `--allow-writes`. No tool can change a whole strip or
  a model this CLI does not know, edit a schedule, a room or a group, change an account or firmware, or
  reach `tplc api`.
- **Security fix:** a failed request's error no longer carries its URL. The device passthrough and the
  Kasa groups call put the session token in the query string, so a connection error printed the token,
  and `--json` carried it.
- A gateway's error page no longer reaches an error message: a non-2xx from the device passthrough or
  the account cloud says its status alone, since the page could repeat the URL and its token.
- A refresh with no token in the answer is an error, not an empty session.
- Token renewal runs under a lock every `tplc` process shares, reads the session again there, and writes
  it once before the request, so a server and a terminal cannot spend one refresh token twice.
- Releases are cut by pushing a tag, no longer by merging a version bump. They attach
  `tplc-<version>.mcpb`, a Claude Desktop extension, and a release is published only once every asset
  is there.
- `self-update` says to restart a running `tplc mcp --http` and to install the new `.mcpb`.
- `auth logout --forget` also removes the connector token. If that fails it still clears the config,
  exits nonzero, and with `--json` reports `connector_token_removed: false` on its `auth-logout/v1`
  document. A running `tplc mcp --http` keeps its token until it stops, and the message says so. A
  login waits for a renewal in flight.
- A strip the cloud lists as offline no longer costs a tool its wait or stops a write by name. A result
  names it. A write that reaches its command 75 s or more after the call began sends nothing and says
  so, so a keychain prompt answered late cannot switch a device.
- `tplc rooms` caches the Tapo app-server URL under the same lock, over the session as it is then. It
  used to store the copy it had read, which could restore a retired refresh token or a logged-out
  session.
- cli-common v0.23.0, up from v0.15.1. The minimum Rust version is 1.89.

## 0.2.3 — 2026-11-03

- `NO_COLOR=1` no longer fails every command with a usage error (exit 2):
  any non-empty `NO_COLOR` disables color, per no-color.org. Built on
  cli-common v0.15.1 (was v0.8.0), whose `CommonArgs` read `NO_COLOR` as a
  clap boolean.
- The patched TLS stack (rustls 0.23.46, rustls-webpki 0.103.16,
  webpki-roots 1.0.11) and tokio 1.54.1, from the lockfile.
- `make install` builds from `Cargo.lock` (`cargo install --locked`).

## 0.2.2 — 2026-09-14

- A Tapo device the **Kasa** cloud lists (it lists them too, typed
  `SMART.TAPO*`, and wins the cross-cloud de-duplication) now has its
  base64 alias decoded like one the Tapo cloud lists, so the Tapo lock
  shows as "Front Door Lock" everywhere instead of `RnJvbnQgRG9vciBMb2Nr`
  in `devices list`, name resolution and `rooms devices`.

## 0.2.1 — 2026-09-10

- `rooms devices` rows now join with Google Home: one of Tapo's own devices
  carries the MAC without separators as `id` (what Google's Tapo integration
  reports as `partner_device_id`); a Kasa device shared into the Tapo app
  keeps its Kasa id. Found by the first live `ghome audit` fed by Tapo rooms.
- The Tapo cloud's base64-encoded aliases are decoded once, where the device
  list enters the program, so `devices list`, name resolution and `rooms
  devices` all show (and accept) the real name.
- One of Tapo's own devices in no room keeps its `device-rooms/v1` row with
  `room` omitted (smart-home/v1 now allows it), so `ghome audit` can report
  the gap as `unfiled`. Roomless Kasa devices shared into Tapo stay out.

## 0.2.0 — 2026-09-10

Conforms to [piekstra-cli spec v1](https://github.com/piekstra/cli-common)
(cli-common v0.8.0). **Breaking** for anyone scripting 0.1:

### Output default flipped
- **Text is the default; `--json` emits JSON.** 0.1 printed JSON by default
  and took `-t/--table` for text. `-t/--table` is still accepted (hidden,
  no-op, a stderr note) for one major version. Pipelines must add `--json`:
  `tplc groups devices --json | ghome audit --expect -`.
- Every JSON document carries `"schema": "<name>/v1"`; lists are
  `{"items": [...]}` (`device-list/v1`, `schedule-rule-list/v1`, …) instead
  of bare arrays; errors are `{"error": {"code", "message"}}` on stdout (in
  `--json`) plus a line on stderr, instead of a JSON object on stderr.
- Nulls are omitted; `light-state/v1` and `device-time/v1` are typed.

### Exit codes renumbered (SPEC §1.5)
| condition | 0.1 | 0.2 |
|---|---|---|
| auth required / expired / MFA parked | 2 | **3** |
| device (or rule, room) not found | 3 | **4** |
| cloud / device / HTTP error | 1 | **5** |
| usage error, unsupported operation | 1 | **2** |
| confirmation required (room writes, non-interactive) | — | **6** |

### Commands
- `login|logout|status` → **`auth login|logout|status`** (old spellings kept
  as hidden aliases). `auth login` takes the family flags (`--stdin`,
  `--from-env <VAR>`, `--overwrite`, `--non-interactive`, `--no-verify`
  (no-op)) plus `--username` and the resumable `--mfa-code`. A login parked
  for MFA now **exits 3** with the `mfa_required` document (was exit 0).
  `--overwrite` is required only to replace another account's session.
- `auth logout --forget` also clears the config identity. `auth
  set-credential` exists for surface uniformity and is always a usage error.
- New: `config path|show|set|unset` (`username`, `default_cloud`),
  `self-update [--check] [-y]`, `completions <shell>`, bare `info`
  (cli-info/v1). `info sysinfo|network|time <DEVICE>` are unchanged.
- New: **`rooms list|devices|move|create|rename|delete`** over the Tapo
  app's homes and rooms (decoded from the Tapo Android app; `docs/api.md`).
  Writes confirm unless `--force`, exit 6 non-interactively, and are read
  back. `rooms devices` emits `device-rooms/v1` like `groups devices`.
- `api --cloud` is now a value enum (`kasa|tapo`) with `$TPLC_CLOUD` and
  `config set default_cloud` fallbacks; `api-response/v1` adds `cloud` and
  `method`.
- `devices list` gains `ls`; `schedule list` gains `ls`; schedule rules
  carry readable `enabled/action/time/days` beside the raw rule.
- `schedule add/edit` validate flags before resolving the device; `light`
  and `energy --month` ranges are enforced by clap.
- `groups devices` / `rooms devices` omit `name` when unknown (never null).
- `config show|set|unset` emit `config/v1` (the effective config).
- `-v` never prints the password, tokens or MFA codes: request bodies are
  redacted before logging (0.1 printed the login body verbatim).

### Keychain
- Service renamed `tplc` → **`piekstra.tplc`**; the `session` and
  `mfa_pending` items are migrated on first read (old → new → delete), as is
  the 0.1.0 eight-item layout. Expect one macOS prompt for the migration.
- The session now caches the Tapo app-server host for 24h (additive field).

### Fixed
- `getDeviceList` returning a non-zero cloud error was read as an empty
  account; it is now an upstream error (exit 5).
- Two devices with the same name (or partial match) are reported as
  ambiguous instead of silently picking the first — for writes especially.

### Build & release
- `Makefile` (`make verify` = fmt + clippy + tests + smoke; signed builds),
  family CI (Linux verify, macOS smoke, cargo-audit + gitleaks), auto-release
  on version bump with `tplc-<target>.tar.gz` + `.sha256` assets for
  aarch64/x86_64 macOS and x86_64 Linux; the Homebrew tap formula follows.
  `version.txt`, `scripts/bump-version.sh` and `scripts/pre-commit` are gone
  (superseded by the Cargo version and `make verify`). aarch64 Linux and
  Windows builds are dropped.
- Dependencies: `tabled`, `dialoguer`, `keyring` (direct) replaced by the
  `pk-cli-*` crates; `wiremock`/`tempfile` dev-deps removed.

## 0.1.x

Pre-spec: JSON by default, `--table`, exit codes 1–4, keychain service
`tplc` (eight items, later one).
