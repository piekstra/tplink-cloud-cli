# The TP-Link clouds, the credential path, and the traps

What `tplc` talks to, how it authenticates, and the places these APIs mislead
you. TP-Link publishes none of it; everything here is reverse-engineered
(from the Kasa/Tapo Android apps and the
[tplink-cloud-api](https://github.com/piekstra/tplink-cloud-api) Python
library) and dated so the next person can tell what may have rotted.

There are **three** clouds behind one TP-Link account:

| Cloud | Host | Auth | What lives there |
|---|---|---|---|
| v2 account cloud (Kasa) | `https://n-wap.tplinkcloud.com` → regional `appServerUrl` | HMAC-signed requests + `token` query | login, MFA, refresh, `getDeviceList`, device passthrough, any method by name |
| v2 account cloud (Tapo) | `https://n-wap.i.tplinkcloud.com` → regional | same, Tapo app keys | same, for Tapo-registered devices; `getAppServiceUrl` |
| Kasa IoT cloud | `https://api.tplinkra.com` | Kasa token in query + body | device groups (the Kasa app's rooms) |
| Tapo NBU app-server | `{region}-app-server.iot.i.tplinknbu.com` (resolved) | `Authorization: ut\|<tapo token>` | homes, rooms, device→room membership |

## The v2 account cloud

### Signing (`src/api/signing.rs`)

Every request to the account cloud carries two headers computed over the
exact JSON body bytes:

```
Content-MD5:      base64(md5(body))
X-Authorization:  Timestamp=9999999999, Nonce=<uuid4>, AccessKey=<app key>, Signature=<hex hmac-sha1>
   where the HMAC key is the app's secret key and the signed string is
   "<content_md5>\n9999999999\n<nonce>\n<url path>"
```

The timestamp is the literal `9999999999` (the app hard-codes it). The
access/secret keys are per-app constants shipped in the APKs
(`src/api/cloud_type.rs`): they identify the app, not the user, and are
public. The URL path in the signature is the request path (`/api/v2/account/
login`, or `/` for the Kasa method endpoint). Every request also carries the
app-identity query string: `appName`, `appVer=3.4.451`, `termID`, `ospf`,
`brand`, `locale`, `model`, `termName`, `termMeta`, and — once logged in —
`token`. User-Agent is `Dalvik/2.1.0 (Linux; U; Android 14; Pixel Build/UP1A)`.

TLS: the cloud's chain is pinned to `certs/tplink-ca-chain.pem`, added as an
extra root (the system store still applies).

### Responses

Always `{"error_code": <int>, "result": {...}?, "msg": "..."?}` with HTTP
200. Codes that matter:

| code | meaning | `tplc` |
|---|---|---|
| 0 | ok | |
| -20601 | wrong email/password | auth (3) |
| -20675 | account locked (too many attempts) | auth (3) |
| -20677 | MFA code required | parks / prompts |
| -20651 | token expired | refresh once, then retry |
| -20655 | refresh token expired | auth (3): `auth login` |
| -20104 | malformed request / unknown parameter | upstream (5) |

**Trap — the inner error.** `login` can answer `error_code: 0` with the
failure inside `result`: `{"errorCode": "-20677", "errorMsg": "..."}`, the
code as a *string* or an int. `TPLinkApi::login` checks both levels.

### Login, MFA, refresh (`src/api/client.rs`)

1. `POST {host}/api/v2/account/getAccountStatusAndUrl`
   `{"appType", "cloudUserName"}` → `result.appServerUrl`, the account's
   **regional** host (e.g. `https://n-use1-wap.tplinkcloud.com`). Every
   later call goes there; it is stored in the session.
2. `POST {regional}/api/v2/account/login` with `cloudPassword`,
   `cloudUserName`, `terminalUUID` (a UUID the CLI generates once per login
   and keeps as `term_id`), `refreshTokenNeeded: true`, `platform:
   "Android"`, `terminalName/Meta: "Pixel"` → `result.token`,
   `result.refreshToken`.
3. MFA: `-20677` (with `result.mfaType`, e.g. `verifyCodeLogin`) means
   TP-Link emailed a code. `POST {regional}/api/v2/account/checkMFACodeAndLogin`
   `{"appType", "cloudUserName", "cloudPassword", "code", "terminalUUID"}`
   → same token result. **The code is bound to the `terminalUUID` that
   asked for it**: presenting it with a fresh UUID fails. That is why a
   non-interactive login *parks*: `tplc` stores `{username, term_id, cloud,
   kasa tokens so far}` as the `mfa_pending` keychain item, exits 3 with an
   `mfa_required` document, and `--mfa-code` resumes with the same UUID.
   Kasa and Tapo each run their own login with the same UUID; each may ask
   for a code (the Tapo one is best-effort: an account without Tapo devices
   still logs in).
4. Refresh: `POST {regional}/api/v2/account/refreshToken`
   `{"appType", "refreshToken", "terminalUUID"}` → new `token` (and
   possibly a new `refreshToken`). Done once, on `-20651`, then the call is
   retried; `-20655` means log in again.

The session `tplc` stores (one keychain item, `piekstra.tplc/session`):
Kasa `token`/`refresh_token`/`regional_url`, `username`, `term_id`, the Tapo
trio, and (additive, 2026-09) the Tapo app-server URL with its expiry.

### `getDeviceList` and the method passthrough

Kasa's account cloud takes `{"method": "<name>", "params": {...}}` posted to
the regional host's **root** (`/`), signed with path `/`. `getDeviceList` →
`result.deviceList[]` of `{deviceId, alias, deviceModel, deviceType,
deviceName, appServerUrl, status (1 = online), deviceMac, hwId, oemId, …}`
(`src/models/device_info.rs`; fixture `tests/fixtures/get_device_list_response.json`).
Tapo's lists the same shape from its own regional host. A device registered
in both clouds appears twice; `tplc` keeps the Kasa row.

`tplc api <METHOD-NAME> [--params JSON] [--cloud]` is exactly this envelope
with any method name — the escape hatch for what the CLI doesn't model.

**Trap — errors used to read as empty.** A non-zero `error_code` on
`getDeviceList` other than token-expired was once treated as "no devices";
since 0.2.0 it is an upstream error.

### Device passthrough (`src/api/device_client.rs`)

Commands reach a device through the cloud, which relays JSON to it:

| cloud | request | path |
|---|---|---|
| Kasa | `{"method": "passthrough", "params": {"deviceId", "requestData": "<JSON string>"}}` | `/` on the device's `appServerUrl` |
| Tapo | `{"deviceId", "requestData": "<JSON string>"}` | `/api/v2/common/passthrough` |

`requestData` is the device's own protocol, **double-encoded** (a JSON
string inside the JSON body); the answer comes back the same way in
`result.responseData`. The device protocol is `{"<service>": {"<method>":
<params>}}`, e.g. `{"system": {"get_sysinfo": null}}`,
`{"system": {"set_relay_state": {"state": 1}}}`,
`{"smartlife.iot.smartbulb.lightingservice": {"transition_light_state":
{"on_off": 1, "brightness": 50}}}`, `{"emeter": {"get_realtime": null}}`,
`{"schedule": {"get_rules": {}}}`, `{"netif": {"get_stainfo": null}}`,
`{"time": {"get_time": {}}}`, `{"system": {"set_led_off": {"off": 1}}}`.
Each answer carries `err_code`.

- **Outlets (children).** Strips (HS300, KP303, KP200, KP400, EP40) list
  their outlets in `get_sysinfo.children[]` (`{id, alias, state, on_time}`;
  an outlet id is the parent's `deviceId` + two digits). To address one,
  add `"context": {"child_ids": ["<child id>"]}` to the request; the answer
  for `get_sysinfo` then has the matching child under `children[]`, which
  `Device::passthrough` unwraps. Power state is `relay_state` on a plug,
  `state` on an outlet, `light_state.on_off` on a bulb.
- **LED is inverted:** `set_led_off {"off": 1}` turns the indicator *off*.
- **Energy units:** newer firmware reports `voltage_mv`, `current_ma`,
  `power_mw`, `total_wh`; older reports `voltage`, `current`, `power`,
  `total` in base units. `CurrentPower::from_json` reads either name but
  does not convert — a value from an old device is in V/A/W/kWh. Same for
  `energy_wh` vs `energy` in the daily/monthly lists.
- **Schedule rules:** `stime_opt` 0 = clock time (`smin` minutes past
  midnight), 1 = sunrise, 2 = sunset; `sact` 1 = on, 0 = off; `wday` is
  seven 0/1 flags Sunday-first; `etime_opt: -1` means no end action.
- Timeouts: passthrough waits up to 600 s (the cloud holds the request
  while it wakes the device); everything else 15–30 s.

## The Kasa IoT cloud — device groups (`src/cli/groups.rs`)

The Kasa app's rooms are "device groups" on a different cloud,
`https://api.tplinkra.com`, authenticated with the **Kasa account token**:

```
POST /v1/device-groups/listDeviceGroups?token=<kasa token>&terminalId=<term_id>&clientId=46a4d58b-6279-432c-ae23-e115c2db8354&requestId=<uuid>
{"requestId": "<uuid>", "module": "device-groups", "method": "listDeviceGroups",
 "iotContext": {"userContext": {"email", "accountToken": "<kasa token>", "terminalId",
                                "app": {"appType": "Kasa_Android", "appClientId": "<clientId>"}}},
 "data": {"paginator": {"from": 0, "pageSize": 100},
          "uri": "com.tplinkra.devicegroups.impl.ListDeviceGroupsRequest"}}
```

No HMAC. Success is `{"status": "SUCCESS", "data": {"listing": [{id, alias,
type, items: [{id: <deviceId>}, …]}]}}`. The `clientId` is the Kasa app's
(public). An account that keeps its rooms in the Tapo app returns an empty
listing here — use `rooms` instead.

## The Tapo NBU app-server — homes and rooms (`src/api/nbu.rs`)

Decoded 2026-09-10 from the Tapo Android app 3.20.753 (`FamilyApi`,
`ThingApi`, the NBU header interceptor and the app-server map). Where the
app was the only witness, the item is marked *inferred*.

### Host resolution

The host is per account and **not** hard-coded. The app asks the v2 Tapo
cloud (signed like login, `token` in the query):

```
POST {tapo regional}/api/v2/common/getAppServiceUrl
{"serviceIds": ["nbu.iot-app-server.app-v2"]}
→ {"error_code": 0, "result": {"regionCode": "us",
     "serviceUrls": {"nbu.iot-app-server.app-v2": "https://use1-app-server.iot.i.tplinknbu.com"}}}
```

The app caches the answer for 24 h; `tplc` does the same, in the session
(`tapo_app_server_url` / `tapo_app_server_expires_at`).

### Headers (every NBU request; no HMAC)

```
Authorization: ut|<tapo v2 login token>
app-cid:       app:TP-Link_Tapo_Android:<term_id>
x-app-name:    TP-Link_Tapo_Android
x-app-version: 3.20.753
x-term-id:     <term_id>
x-ospf:        Android 14
x-net-type:    wifi
x-strict:      0
x-locale:      en_US
User-Agent:    TP-Link_Tapo_Android/3.20.753(Pixel/;Android 14)
Content-Type:  application/json;charset=UTF-8      (with a body)
```

The `ut|` prefix is literal. The same `term_id` as the v2 login is used.
Responses are **bare JSON** (no `error_code` envelope); failures are HTTP
statuses with `{"code", "message"}`; 401/403 → refresh the Tapo token once.
TLS pinning (`*.iot.i.tplinknbu.com`) is client-side in the app; `tplc`
uses its normal trust store plus the pinned TP-Link chain.

### Endpoints

Paged endpoints take `page` (0-based) and `pageSize` and answer
`{"page", "pageSize", "total", "data": [...]}`; `tplc` walks pages until
`data` is exhausted or `total` is reached (fixture shapes in
`tests/fixtures/nbu_*.json`).

| | method | path | body / notes |
|---|---|---|---|
| homes + rooms | GET | `/v1/families?page=0&pageSize=20` | `data[]: {id, name, default, rooms: [{id, name, avatarUrl}]}` |
| devices with rooms | GET | `/v2/things?page=0&pageSize=20&includePcDevice=true&includeKasaShareDevices=true&includeMatterDevice=true&includeExternalVendorDeviceInfo=true` | `data[]: ThingInfo {thingName (= deviceId), familyId, roomId, nickname, deviceModel, deviceType, category, status, mac, …}`; `roomId: null` = unassigned. Omitting `deviceTypes` returns everything (*inferred*) |
| move device(s) | POST | `/v1/families/thing-settings` | `{"familyId", "roomId", "thingNames": ["<deviceId>", …]}` → empty 2xx |
| create / rename room | PUT | `/v1/families/{familyId}/rooms` | `{"id": "<8 chars [A-Za-z0-9]>", "name"}` — an upsert on `id`: a new id creates, an existing one renames (*upsert inferred from the app's two call sites*) → `{id, name, avatarUrl}` |
| delete room | DELETE | `/v1/families/{familyId}/rooms/{roomId}` | empty |
| rooms only | GET | `/v1/families/{familyId}/rooms?page&pageSize` | rooms are already embedded in `/v1/families` |
| room order | POST | `/v1/families/{familyId}/rooms/order` | `{"roomIds": [...]}` (not used) |
| device order in a room | GET/POST | `/v1/families/{familyId}/thing-order`, `…/rooms/{roomId}/thing-order` | display order, not membership (not used) |

`nickname` is the device name, which Tapo firmware reports base64-encoded;
the cloud passes some through as-is. `Thing::display_name` decodes a value
that is valid base64 of clean text and keeps anything else verbatim. `tplc
rooms devices` prefers the account cloud's `alias` for the same `deviceId`.

**Trap — the account cloud is encoded too.** The Tapo cloud's v2
`getDeviceList` hands `alias` through base64 as well
(`RnJvbnQgRG9vciBMb2Nr`), while Kasa's is as typed. `DeviceInfo::from_cloud`
decodes it once, where the list enters the program, so `devices list`, name
resolution and `rooms devices` all see the real name; decode nowhere else.

**Trap — Google's id is the MAC.** Google Home's Tapo integration reports
`partner_device_id` as the MAC without separators, not `thingName`, so a
`device-rooms/v1` row keyed on `thingName` silently matches nothing for a
Tapo device (the Kasa integration does use the Kasa device id). Found by a
live `ghome audit`, not by reading the wire shapes.

### What `tplc rooms` does with them

- `rooms list`: `/v1/families` + `/v2/things` (for counts).
- `rooms devices`: the join `things.roomId → families[].rooms[]`, emitted as
  `device-rooms/v1`. `id` is what Google Home's `partner_device_id` shows
  for the device: for one of Tapo's own devices the **MAC without
  separators, upper-case** (`mac` → `105A952FAD17`); for a Kasa device
  shared into the Tapo app its Kasa device id (`thingName`). `name` is the
  account cloud's alias over the decoded `nickname`. One of Tapo's own
  devices in no room keeps its row with `room` omitted (smart-home/v1
  allows it, and `ghome audit` reports it as `unfiled`); a roomless
  Kasa-shared device is left out, `groups devices` being its home.
- `rooms move`: gate → resolve the thing and the room (in the thing's own
  home unless `--home`) → confirm → `thing-settings` → **re-read
  `/v2/things`** and require `roomId` to match.
- `rooms create|rename`: gate → `PUT rooms` → re-read `/v1/families` and
  require the room (with the new name) to be there. `create` refuses a
  duplicate name in the same home.
- `rooms delete`: refuses a room that still has things → `DELETE` → re-read
  and require it gone.

**Trap — empty bodies.** Every write above answers with nothing useful. A
2xx is not success; only the read-back is.

**Trap — `x-app-name`.** Live traffic has been seen with `x-app-name: Tapo`
and `app-cid: app:Tapo:…`; the decompiled build sends
`TP-Link_Tapo_Android`. The server accepts both; `tplc` sends the decoded
value.

## Sources

- The `tplink-cloud-api` Python library (`tplinkcloud/client.py`,
  `signing.py`, `device_client.py`) and its WireMock captures — the v2
  cloud, signing, MFA and passthrough shapes.
- Tapo Android 3.20.753 (jadx): `com/tplink/iot/cloud/api/{FamilyApi,
  ThingApi}.java`, `cloud/bean/family/**`, `cloud/bean/thing/common/
  ThingInfo.java`, the NBU interceptor (`pq/a.java`), `oq/{a,b}.java`
  (`ut|` token, `app-cid`), `com/tplink/cloud/api/WebServiceV2Api.java`
  (`getAppServiceUrl`), `PartitionIsolationGetManager` (24 h cache).
- Kasa Android: the IoT cloud `IOTRequest` envelope and client id.
