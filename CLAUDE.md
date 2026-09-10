# CLAUDE.md

The canonical agent guide for this repo is **[AGENTS.md](AGENTS.md)** — read it
first. It covers build/test/lint, layout, conventions, and the safety rules.

Claude Code specifics:

- **Gate on `make verify`.** Don't report a change as done until it's green
  (fmt + clippy `-D warnings` + tests + smoke). Tests are fully offline and
  never read the keychain (a fresh test binary prompts on macOS).
- **Never run `auth login` or any device/room write to "test" it.** They act
  on the owner's real TP-Link account and hardware. Live verification is the
  owner's call, through the installed binary.
- **Secrets:** session tokens live in the OS keychain (`piekstra.tplc`), one
  item. Never print them, put them on argv, or write them to a file. The
  password is never stored.
- **"Deployed" means released + installed.** A change isn't live until the
  release workflow ships it and the binary is installed or `self-update`d.
- **Public repo, private home.** No real emails, device ids, MACs, or tokens
  in any diff — fixtures included (dummies only; `tests/fixtures/README.md`).
