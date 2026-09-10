# Contributing

1. Read [AGENTS.md](AGENTS.md) — it is the house style and the safety rules.
2. `make verify` must be green before a PR. Tests are offline; nothing may
   touch the keychain or the network.
3. New cloud findings (an endpoint, a wire shape, a header, a trap) go in
   `docs/api.md` with a date and a source, so nobody re-derives them.
4. Fixtures are scrubbed captures or documented synthetics: keep the structure
   exact, replace every identifying value with the dummies described in
   `tests/fixtures/README.md`.
5. A new write command must gate (`--force`, exit 6 non-interactively, before
   any I/O), read its result back from the cloud, and take its wire shape
   from the app or the Python library — never from trial requests against a
   live account.
6. Adding a device model: extend `DeviceType` in `src/models/device_type.rs`
   (prefix map, capabilities, display name), add a unit test, update the
   README table.
7. Adding a cloud: a `CloudType` variant with its host, keys, app type and
   passthrough envelope; the login flow in `src/cli/auth.rs` and the session
   in `src/session.rs`.
