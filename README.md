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
cargo install --git https://github.com/piekstra/tplink-cloud-cli --locked
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

## Use with Claude

`tplc mcp` serves five tools over MCP, so Claude can see your Kasa and Tapo devices and, when you ask,
change one light, plug or outlet. There is no hosted service: the tools run on your computer, in your
copy of `tplc`, and call TP-Link's cloud with your session.

| Tool | Does | Changes a device |
| --- | --- | --- |
| `tplink_devices` | What do I have? Name, kind, online, strip, what the tools can change on it, and whether it has a meter, narrowed by `name` | no |
| `tplink_device_state` | Is it on? Online, power, brightness, color or color temperature, a strip's outlets | no |
| `tplink_energy` | What does it draw? Power, voltage, current and total now, or a year's months, or a month's days | no |
| `tplink_power` | Turn one light, plug, wall switch or outlet on or off | **yes** |
| `tplink_light` | Set one light's brightness, color or color temperature, which also turns it on | **yes** |

The two that change a device are not read-only and carry `anthropic/requiresUserInteraction`, so a
client that honors them asks you before every call. Each takes one device by its full name or its id.
Part of a name is refused however unique, so "all" cannot reach the hallway light, and there is no
list of devices, wildcard or group. A whole strip is refused, and the refusal names its outlets: one
call changes one outlet. A model `tplc` does not know is refused too, because a Tapo account can list
a lock or a camera, and an offline device is refused with nothing sent. Each returns the state read
back from the device, about 0.3 and 1.3 seconds later, with `confirmed`: `true` only when that state
shows the change. If it does not, or the read fails, a `note` says so. That is not a failure, and
Claude is told not to send the command again. If the command's own answer is lost, the state read
back settles it the same way, and the `note` says the outcome is unknown until it does. While part of
the device list cannot be read, such as the Tapo cloud or a strip's outlets, a write by name is refused
with nothing sent, because the name could mean a device in the missing part. Its id still works. A
strip the cloud lists as offline is not a missing part: its outlets could not take a command anyway, so
a result names it in `note` and a name shared with one of them reaches the device that is online. A
write that reaches its command 75 seconds or more after the call began sends nothing and says so.

No tool can remove a device, edit a schedule, a room or a group, change a device's LED or firmware,
change your account or share access, and none reaches `tplc api`. Toggling is left to the CLI, so a
repeated call cannot undo itself.

Results are concise by default (`response_format: "detailed"` adds a device's id, model and cloud), 25
records a page with a cursor for more. A field a device left out is absent and never a default, an
answer of the wrong shape is an error, and a device list missing the Tapo cloud or a strip's outlets
says so in `note`. Each page reads TP-Link again, so a device added between pages can repeat or be
skipped: for a count, read one call with `limit` up to 200. Device names are yours; the server tells
Claude to treat them as data, not instructions. A device's MAC address and coordinates, which it
reports alongside its state, never reach a result. Tools wait 15 seconds for each request, where the
CLI waits 600, so a device the cloud is still trying to wake reads as a failure, or a `note`, and not
as a long silence.

The tools are worded for a model, so they differ from `--json`, which passes TP-Link's names through:
`power: "on"` for `relay_state: 1` or `light_state.on_off: 1`, `power_w` for `power_mw`, `voltage_v` for
`voltage_mv`, `current_a` for `current_ma`, `total_kwh` for `total_wh`, `energy_kwh` for `energy_wh`,
and `kind: "other"` for a model the CLI calls a plug because it does not know it.

**Sign in first.** Run `tplc auth login` in a terminal ([Setup](#setup)). Every path reads that session
from the OS keychain (service `piekstra.tplc`), renews an expired token there under a lock every
`tplc` process shares, and asks for nothing itself. When the session ends the tools say so and ask you
to run it again. On macOS a program the keychain has not seen asks once per item, for the read and for
the write a renewal makes; choose Always Allow for each.

**Claude Code.** Add the `tplink-cloud-cli` plugin from the
[piekstra plugin marketplace](https://github.com/piekstra/claude-plugins):

```console
$ claude plugin marketplace add piekstra/claude-plugins
$ claude plugin install tplink-cloud-cli@piekstra-plugins
```

or register the server yourself:

```console
$ claude mcp add tplink -- tplc mcp
```

**Claude Desktop.** Download `tplc-<version>.mcpb` from the
[latest release](https://github.com/piekstra/tplink-cloud-cli/releases/latest) and open it. It has no
settings: install the CLI and sign in first, and the bundled `tplc` reads the same keychain session.
A new version of the extension is a new program to the keychain and can ask again. It runs on macOS,
Linux and Windows.

Any other MCP client that launches a local server can run `tplc mcp`.

**Claude on the web and mobile.** claude.ai reaches tools through a remote connector, so `tplc` serves
the tools from your computer to a connector you deploy into your own Cloudflare account:

```console
$ tplc mcp --new-token            # once: a bearer token, saved in the keychain and printed for the Worker
$ tplc mcp --http --port 8761     # serve on 127.0.0.1 only
```

The connector, [cli-common's `connector/`](https://github.com/piekstra/cli-common/tree/main/connector#tools-from-your-computer-home-modules),
reaches that port through a Cloudflare Tunnel in your account; its README walks through the tunnel, the
VPC service, a launchd agent that keeps the server running, and the `HOME_TPLINK_TOKEN` secret, with the
threat model. `--http` binds 127.0.0.1 only, refuses to start without the token, and refuses any request
whose `Host` is not loopback or that carries an `Origin`.

It serves the three reads only. `--allow-writes` adds `tplink_power` and `tplink_light`, and then
anyone holding the token can change your lights and plugs through your Worker, with no prompt unless
the client asks for one; leave it off unless you want that. Your TP-Link session stays on your
computer, but tool results do not: your device names, states and energy use pass through Cloudflare
and reach Claude ([`SECURITY.md`](SECURITY.md)). While the computer sleeps, the tools answer that it is
offline.

After `tplc self-update`, restart the server: a running one keeps the old code, and its launchd agent
restarts it only when it exits. Its start line names the version.

```console
$ launchctl kickstart -k gui/$(id -u)/com.piekstra.tplc-mcp-http
```

`self-update` does not reach a Claude Desktop extension, which runs its own copy. Install the new
release's `.mcpb` too.

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
