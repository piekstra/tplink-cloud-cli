# tplc — TP-Link Kasa and Tapo devices from the terminal

`tplc` controls TP-Link Kasa and Tapo smart-home devices through the TP-Link
cloud, for people and for agents: list devices, switch them, read energy
meters, drive light strips, manage schedules, and see (and fix) which room
the Tapo app files each device under. One TP-Link login covers both clouds.

Conforms to [piekstra-cli spec v1](https://github.com/piekstra/cli-common)
(`--json` everywhere, standard exit codes, keychain-only secrets).

**Unofficial.** Not affiliated with or endorsed by TP-Link. It speaks the
private API behind the Kasa and Tapo apps, documented in
[docs/api.md](docs/api.md); TP-Link can change or close it at any time. Use
it on your own account, at your own risk.

## Install

```console
brew install piekstra/tap/tplc
# or a release binary: https://github.com/piekstra/tplink-cloud-cli/releases
# or from source:
cargo install --git https://github.com/piekstra/tplink-cloud-cli
```

Later: `tplc self-update` (or `--check` to only look).

## Setup

```console
tplc auth login                 # prompts for email and password (no echo)
tplc auth status                # auth-status/v1; works logged out
```

The password is exchanged for Kasa and Tapo session tokens, stored as one
item in the OS keychain under `piekstra.tplc`; the password itself is never
stored. Headless (the secret never touches argv):

```console
op read "op://Private/TP-Link/password" | tplc auth login --stdin --username you@example.com
TPLC_PASSWORD=… tplc auth login --username you@example.com --non-interactive
tplc auth login --from-env MY_TPLINK_PASSWORD --username you@example.com
```

Identity precedence: `--username` > `$TPLC_USERNAME` > `tplc config set
username …` (written by a successful login) > prompt. Password precedence:
`--stdin` / `--from-env` > `$TPLC_PASSWORD` > prompt.

**MFA.** If TP-Link emails a verification code, an interactive login asks
for it. A non-interactive login *parks* instead: it stores the half-finished
login (bound to the terminal id the code was requested for), prints an
`auth-login/v1` document with `"status": "mfa_required"`, and exits 3.
Resume with the code:

```console
op read … | tplc auth login --stdin --username you@example.com --mfa-code 123456
```

`--overwrite` is only needed to replace a session that belongs to a
*different* account; logging in again as the same account just renews it.
`--no-verify` is accepted for family uniformity and has no effect — the
login is the verification. `auth set-credential` is always a usage error:
there is no raw credential to write (the session is minted by the cloud).

## Usage

```console
tplc devices list                       # every device; outlets of a strip listed too
tplc devices get "Living Room Lamp"     # details + live sysinfo
tplc devices search lamp

tplc power on "Living Room Lamp"
tplc power toggle "Porch Light"
tplc power status "Living Room Lamp"

tplc energy realtime "Kitchen Plug"     # HS110, KP115, KP125, P110, HS300 outlets
tplc energy daily "Kitchen Plug" --year 2026 --month 1
tplc energy monthly "Kitchen Plug"
tplc energy summary                     # devices that have a meter

tplc light brightness "Strip" 75
tplc light color "Strip" --hue 240 --saturation 100
tplc light temp "Strip" 4000
tplc light state "Strip"

tplc schedule list "Porch Light"
tplc schedule add "Porch Light" --action on --sunset --days mon,tue,wed,thu,fri
tplc schedule edit "Porch Light" RULE_ID --disable
tplc schedule delete "Porch Light" RULE_ID
tplc schedule clear "Porch Light"

tplc info sysinfo "Porch Light"         # info network | info time
tplc led off "Porch Light"
```

Devices are addressed by exact name, id, case-insensitive name, or a unique
partial name — in that order; an ambiguous reference lists the candidates.
Both clouds are searched; a device in both is listed once.

### Rooms (Tapo) and groups (Kasa)

The Tapo app keeps homes and rooms on its own cloud; the Kasa app keeps
"device groups" on another. `tplc` reads both and can change the Tapo one:

```console
tplc rooms list                          # room-list/v1: name, devices, home, id
tplc rooms list --home Cabin
tplc rooms move "Office Lamp" --room Office     # prompts; --force to skip
tplc rooms create Loft
tplc rooms rename Loft Attic
tplc rooms delete Attic                  # only when empty
tplc groups list                         # Kasa device groups (read-only)
```

Writes prompt for confirmation unless `--force`; non-interactive runs
(`--json`, a pipe, an agent) exit 6 *before* touching the keychain or the
network, and every write is read back from the cloud before it is reported.

Both `rooms devices` and `groups devices` emit `device-rooms/v1` — every
device with the room the vendor app files it under (a Tapo device in no
room keeps its row with `room` omitted) — which is what
[`ghome audit`](https://github.com/piekstra/google-home-cli) consumes to
check Google Home's rooms against the vendor's:

```console
tplc rooms devices --json | ghome audit --expect - --problems
```

### Raw API

```console
tplc api getDeviceList
tplc api listDeviceGroups --params '{"paginator":{"from":0,"pageSize":50}}'
tplc api getDeviceList --cloud tapo
```

`api` is a **cloud-RPC passthrough**: the TP-Link cloud is called by method
name on the signed v2 endpoint, so this is `api <METHOD-NAME> [--params
JSON] [--cloud kasa|tapo]` rather than the family's HTTP `api <VERB>
<PATH>` form. `--cloud` defaults to `$TPLC_CLOUD`, then `config set
default_cloud`, then `kasa`. See [docs/api.md](docs/api.md).

## Output

Text is the default: key/value blocks for one resource, pipe-delimited
tables for lists. `--json` emits exactly one schema-tagged document per
invocation (`device-list/v1`, `power-state/v1`, `energy-realtime/v1`,
`schedule-rule-list/v1`, `room-list/v1`, `device-rooms/v1`, `api-response/v1`,
…), and errors as `{"error": {"code", "message"}}`. Diagnostics go to
stderr; `-v` shows requests (never tokens); `-q` silences the rest.

The pre-0.2 `-t/--table` flag is accepted as a no-op for one major version.

## Exit codes

0 ok · 1 unexpected · 2 usage · 3 auth (run `tplc auth login`; also a login
parked for an MFA code) · 4 not found (device, rule, room) · 5 upstream
(cloud or device error, device answered nothing) · 6 confirmation required
(a room write without `--force` in a non-interactive run).

## Configuration

`tplc config path|show|set|unset` over `~/.config/tplc/config.json`
(`--config <PATH>` / `$TPLC_CONFIG` override). Keys: `username` (login
default), `default_cloud` (`kasa`|`tapo`, for `api`). Nothing secret lives
there.

## Supported devices

### Kasa

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

### Tapo

| Model | Type | Energy monitoring |
|-------|------|:-:|
| P100 | Mini Smart Wi-Fi Plug | |
| P110 | Mini Smart Wi-Fi Plug | Yes |
| L530 | Smart Wi-Fi Light Bulb | |

Other models still list and switch through the generic passthrough; they are
just not classified.

## For agents

Every command takes `--json`; branch on the exit code, not the message
(3 → run `tplc auth login`; 4 → `tplc devices list`; 5 → retry later).
`tplc info` (cli-info/v1) lists capabilities and profiles. Conventions and
safety rules for working *on* this repo: [AGENTS.md](AGENTS.md).

## Related

- [tplink-cloud-api](https://github.com/piekstra/tplink-cloud-api) — the
  Python library this CLI's cloud client is ported from.
- [google-home-cli](https://github.com/piekstra/google-home-cli) (`ghome`)
  and [govee-cli](https://github.com/piekstra/govee-cli) — the audit
  consumer and the other `device-rooms/v1` producer.
- [cli-common](https://github.com/piekstra/cli-common) — the shared spec and
  crates.

## License

GPL-3.0 — see [LICENSE](LICENSE).
