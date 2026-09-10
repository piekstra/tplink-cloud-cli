# Test fixtures — TP-Link cloud wire shapes

Files the contract tests (`tests/fixture_shapes.rs`) load; nothing is
embedded as a string literal. Two families of shape:

- **v2 account cloud** (`v2_*`, `get_device_list*`): the `{error_code,
  result, msg}` envelope. `get_device_*_response.json` and
  `*_get_sys_info_response.json` are device **passthrough** answers, whose
  `result.responseData` is a JSON *string* (double-encoded) holding the
  device's own `{"<service>": {"<method>": {...}}}` block.
- **Tapo NBU app-server** (`nbu_*`): bare `{page, pageSize, total, data}`
  pages, no envelope (see `docs/api.md`, "Rooms").

## Provenance

- `get_device_list`, `tapo_get_device_list`, `v2_login`, `v2_account_status`,
  `get_device_emeter_*`, `get_device_net_info`, `get_device_time`,
  `get_device_schedule_rules`, `hs103/hs300/kl430_get_sys_info`: copied from
  the [tplink-cloud-api](https://github.com/piekstra/tplink-cloud-api) Python
  library's WireMock captures and **re-scrubbed** here (2026-09-10) — the
  structure is untouched, every identifier is replaced.
- `v2_login_mfa_required`, `v2_get_app_service_url`, `nbu_families`,
  `nbu_things`: **synthetic**, assembled from the shapes the client code and
  the decoded Tapo app expect (`docs/api.md`). Replace with a scrubbed live
  capture when one exists; keep the file names.

## Scrubbing policy (enforced by `fixtures_carry_only_dummy_identities`)

Every identity-bearing value must be an obvious dummy:

- device ids (40 hex chars): zero-padded counters, `0000…0001`; an outlet's
  id is its parent's id plus two digits (`…000300`);
- `oemId` / `hwId` / `fwId`: all zeros; MACs: `00:00:00:00:00:0N` /
  `00000000000N`;
- emails: `@example.com` only; `accountId`: `000000`;
- tokens: `kasa-token-dummy`, `tapo-refresh-dummy`, …;
- device coordinates (`longitude_i` / `latitude_i`): `0`;
- `appServerUrl`: `http://127.0.0.1:8080`; NBU ids: `FAM0000N` / `ROOM000N`;
- names (`alias`, `ssid`, room names) are generic and carry no identity.

Raw captures go under `/captures/` (git-ignored), never in the tree.
