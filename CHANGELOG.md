# Changelog

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
