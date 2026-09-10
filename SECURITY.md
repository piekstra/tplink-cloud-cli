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

To report a vulnerability, open a private security advisory on the GitHub
repository rather than a public issue.
