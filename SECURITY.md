# Security

`tplc` holds TP-Link session tokens (Kasa and Tapo access + refresh tokens,
the terminal id they are bound to, and the account's regional and app-server
URLs) as a single item in the OS keychain under `piekstra.tplc`. A login
parked for an MFA code is a second, temporary item. The account password is
used for the login exchange and dropped; it is never stored, logged, or
accepted on the command line.

The tokens are as powerful as the Kasa and Tapo apps: they can list and
control every device on the account and, through the raw `api` passthrough,
call any cloud method. `tplc auth logout` removes them locally; the TP-Link
account's security settings can revoke sessions server-side.

`--verbose` prints request URLs, method names, and status codes — never
tokens or signatures. The app-level signing keys in `src/api/cloud_type.rs`
identify the apps to the cloud, not the user; they are public.

TP-Link's answers name the hosts its tokens and the password go to. `tplc` sends them only to `https` on
`tplinkcloud.com`, `tplinknbu.com` or `tplinkra.com`, refuses any other address as an upstream failure,
and follows no redirect off a host.

## The MCP server

`tplc mcp` serves five tools to an MCP client (Claude Code, Claude Desktop) over stdio; the server
opens no port unless run with `--http` (below). **What the tools return goes to Claude on every
path**, as part of your conversation: your device names and kinds, what each device reports about its
power and color, its energy use, and with `response_format: "detailed"` its id and model. A device
also reports its MAC address and coordinates, and no tool returns them.

Three tools read. `tplink_power` and `tplink_light` change one device each, whose full name or id the
call must carry, and they refuse a whole strip, a model this CLI does not know (a Tapo account can list
a lock or a camera) and an offline device, with nothing sent. They are not read-only and carry
`anthropic/requiresUserInteraction`, so a client that honors them asks before every call. They can send
only a power, brightness, color or color-temperature command, and return the state read back. A tool
cannot reach `tplc api`, schedules, rooms, groups, the LED, a toggle or any credential write except the
renewal of a token. `src/mcp/tests.rs` holds the definitions to that, and the requests the live path
makes against a stub. An approval is the client's: `--http` with `--allow-writes` serves them to
whoever holds the token, and a client that ignores the annotation does not ask.

A name in a result can come from anyone who has named a device on the account, so the server tells
Claude to treat names as data and to change a device only on the user's direct request.

The server reads the session on each call and holds it no longer than the call, so a new login applies
on the next. A tool never prompts for a password, moves a legacy session or creates a keychain item:
no session is an error asking the user to run `tplc auth login` in a terminal. It does write the
session back after a renewal, because a refresh can retire the old token, and it does that under a lock
in `~/.config/tplc/token-refresh.lock` shared with every other `tplc` process. On macOS a binary the
keychain has not granted raises a prompt on a call, and a server left unattended waits on it. A write
that reaches its command 75 s or more after the call began, by the wall clock or by a clock that stops
while a Mac sleeps, sends nothing and says so, so a prompt answered late cannot switch a device. The Claude Desktop extension is such a binary, with a grant of
its own.

A write by name is refused while a cloud's list or an answering strip's outlets are missing, because
the name could mean a device in the missing part. A strip the cloud lists as offline does not count: a
result names it in `note`, and a name shared with an outlet on it reaches the device that is online.

A session too long for one credential on Windows is held in pieces: Windows limits an item to 2,560
bytes (1,280 UTF-16 units), and `pk-cli-secrets` splits a longer value and reads it back as one. A logout
after a downgrade is repeated from a current build, since an older build's logout leaves the parts.

`tplc mcp --http --port <PORT>` serves over HTTP for the user's own Cloudflare Worker, which reaches it
through the user's own Cloudflare Tunnel (cli-common's `connector/`). It binds 127.0.0.1 only, with no
flag for another address, and refuses to start without a bearer token of 32 or more characters, from
`TPLC_MCP_TOKEN` or the keychain item `mcp-http-token` (`piekstra.tplc`), which `tplc mcp --new-token`
makes from 256 random bits and replaces only with `--overwrite`. Every request must carry the token
(compared in constant time) and a `Host` of `127.0.0.1` or `localhost`, and must not carry an `Origin`;
there are no CORS headers. It serves the three reads only, and a write is neither listed nor callable,
unless started with `--allow-writes`. The TP-Link session stays on your computer: the Worker sends tool
calls and receives results, and never sees it.

**Tool results also cross Cloudflare on this path**, through your tunnel and your Worker, on their way
to Claude, and the Worker holds a copy of the token as `HOME_TPLINK_TOKEN`. Who holds what, and what a
leaked token can do, is in the connector's
[threat model](https://github.com/piekstra/cli-common/blob/main/connector/README.md#threat-model). With
`--allow-writes`, a leaked token can also turn your lights and plugs on and off.

`tplc auth logout --forget` removes the session, `mcp-http-token` and the config. A server already
running keeps the token it started with and goes on answering, with no session, until it is stopped;
plain `auth logout` removes the session and keeps the token. To revoke the connector's access and keep
serving, run `tplc mcp --new-token --overwrite` and restart the server (`launchctl kickstart -k
gui/$(id -u)/com.piekstra.tplc-mcp-http`); the old token works until the restart. To stop serving, run
`launchctl bootout gui/$(id -u)/com.piekstra.tplc-mcp-http` and stop the tunnel.

To report a vulnerability, open a private security advisory on the GitHub
repository rather than a public issue.
