//! Offline surface tests: flags, exit codes, and the JSON output contract.
//!
//! No network. Nothing here reads the OS keychain by default: `cargo test`
//! produces an ad-hoc-signed binary that macOS treats as a new identity, so
//! any keychain read would prompt (and, on a machine with a legacy `tplc`
//! session, would *migrate* it from a test binary). Every command under test
//! either needs no credential or fails validation before looking for one.
//! The session lives only in the keychain, so "credentialed reads exit 3
//! before the keychain" cannot be asserted without touching it; those
//! assertions run only with `TPLC_TEST_KEYCHAIN=1` on a machine whose
//! keychain holds no `tplc` session (see AGENTS.md).

use assert_cmd::Command;
use predicates::prelude::*;

fn tplc() -> Command {
    let mut c = assert_cmd::cargo::cargo_bin_cmd!("tplc");
    // A config path that does not exist: no username, no default cloud.
    c.env("TPLC_CONFIG", "/nonexistent/tplc-test-config.json");
    for var in ["TPLC_USERNAME", "TPLC_PASSWORD", "TPLC_CLOUD", "NO_COLOR"] {
        c.env_remove(var);
    }
    c
}

fn json_stdout(out: &assert_cmd::assert::Assert) -> serde_json::Value {
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {stdout}"))
}

fn keychain_tests_enabled() -> bool {
    std::env::var("TPLC_TEST_KEYCHAIN").is_ok_and(|v| v == "1")
}

#[test]
fn help_lists_the_standard_and_domain_surface() {
    tplc()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("auth"))
        .stdout(predicate::str::contains("config"))
        .stdout(predicate::str::contains("devices"))
        .stdout(predicate::str::contains("power"))
        .stdout(predicate::str::contains("energy"))
        .stdout(predicate::str::contains("light"))
        .stdout(predicate::str::contains("schedule"))
        .stdout(predicate::str::contains("rooms"))
        .stdout(predicate::str::contains("groups"))
        .stdout(predicate::str::contains("api"))
        .stdout(predicate::str::contains("self-update"))
        .stdout(predicate::str::contains("completions"))
        .stdout(predicate::str::contains("info"))
        // The global flags from CommonArgs.
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--quiet"))
        .stdout(predicate::str::contains("--no-color"))
        .stdout(predicate::str::contains("--config"));
}

#[test]
fn every_subcommand_help_renders() {
    // Catches clap runtime panics (a subcommand flag colliding with a global
    // like -q) that only surface when the subtree is built.
    for args in [
        vec!["auth", "--help"],
        vec!["auth", "login", "--help"],
        vec!["auth", "status", "--help"],
        vec!["auth", "logout", "--help"],
        vec!["auth", "set-credential", "--help"],
        vec!["config", "--help"],
        vec!["config", "set", "--help"],
        vec!["devices", "--help"],
        vec!["devices", "list", "--help"],
        vec!["devices", "get", "--help"],
        vec!["devices", "search", "--help"],
        vec!["power", "on", "--help"],
        vec!["power", "off", "--help"],
        vec!["power", "toggle", "--help"],
        vec!["power", "status", "--help"],
        vec!["energy", "realtime", "--help"],
        vec!["energy", "daily", "--help"],
        vec!["energy", "monthly", "--help"],
        vec!["energy", "summary", "--help"],
        vec!["light", "brightness", "--help"],
        vec!["light", "color", "--help"],
        vec!["light", "temp", "--help"],
        vec!["light", "state", "--help"],
        vec!["schedule", "list", "--help"],
        vec!["schedule", "get", "--help"],
        vec!["schedule", "add", "--help"],
        vec!["schedule", "edit", "--help"],
        vec!["schedule", "delete", "--help"],
        vec!["schedule", "clear", "--help"],
        vec!["info", "--help"],
        vec!["info", "sysinfo", "--help"],
        vec!["info", "network", "--help"],
        vec!["info", "time", "--help"],
        vec!["led", "--help"],
        vec!["rooms", "--help"],
        vec!["rooms", "list", "--help"],
        vec!["rooms", "devices", "--help"],
        vec!["rooms", "move", "--help"],
        vec!["rooms", "create", "--help"],
        vec!["rooms", "rename", "--help"],
        vec!["rooms", "delete", "--help"],
        vec!["groups", "list", "--help"],
        vec!["groups", "devices", "--help"],
        vec!["api", "--help"],
        vec!["self-update", "--help"],
        vec!["completions", "--help"],
        // Hidden aliases kept for one major version.
        vec!["login", "--help"],
        vec!["logout", "--help"],
        vec!["status", "--help"],
    ] {
        tplc().args(&args).assert().success();
    }
}

#[test]
fn info_emits_cli_info_v1() {
    let out = tplc().arg("info").assert().success();
    let v = json_stdout(&out);
    assert_eq!(v["schema"], "cli-info/v1");
    assert_eq!(v["name"], "tplc");
    assert_eq!(v["spec"], "piekstra-cli/1");
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["auth"]["required"], true);
    assert_eq!(v["auth"]["method"], "password");
    assert_eq!(v["auth"]["login_hint"], "tplc auth login");
    let caps: Vec<&str> = v["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    for cap in [
        "devices", "power", "energy", "light", "schedule", "info", "led", "rooms", "groups", "api",
    ] {
        assert!(caps.contains(&cap), "missing capability {cap}");
    }
    assert_eq!(v["profiles"][0], "smart-home/v1");
    // `info --json` is the same document.
    let again = tplc().args(["--json", "info"]).assert().success();
    assert_eq!(json_stdout(&again), v);
}

#[test]
fn version_flag() {
    tplc()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn usage_error_exits_2_with_json_error_dto() {
    let out = tplc()
        .args(["--json", "config", "set", "bogus_key", "x"])
        .assert()
        .code(2);
    let v = json_stdout(&out);
    assert_eq!(v["error"]["code"], "usage");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("unknown config key"));
    // Text mode: nothing on stdout, the message on stderr.
    tplc()
        .args(["config", "set", "default_cloud", "hue"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("kasa"));
}

#[test]
fn config_show_and_path_work_without_a_config_file() {
    let out = tplc().args(["--json", "config", "show"]).assert().success();
    assert_eq!(json_stdout(&out), serde_json::json!({}));
    tplc()
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "/nonexistent/tplc-test-config.json",
        ));
    // --config overrides the env.
    tplc()
        .args(["--config", "/tmp/elsewhere.json", "config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains("/tmp/elsewhere.json"));
}

#[test]
fn deprecated_table_flag_is_accepted_as_a_no_op() {
    // `-t/--table` was the pre-spec way to ask for text; text is the default
    // now, so the flag must still parse (hidden) and change nothing.
    let plain = tplc().arg("info").assert().success();
    let flagged = tplc()
        .args(["--table", "info"])
        .assert()
        .success()
        .stderr(predicate::str::contains("deprecated"));
    assert_eq!(json_stdout(&plain), json_stdout(&flagged));
    tplc()
        .args(["-t", "--quiet", "info"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    assert!(
        !String::from_utf8(tplc().arg("--help").output().unwrap().stdout)
            .unwrap()
            .contains("--table"),
        "--table is hidden from help"
    );
}

#[test]
fn api_validates_params_and_cloud_before_any_credential() {
    let out = tplc()
        .args(["--json", "api", "getDeviceList", "--params", "{not json"])
        .assert()
        .code(2);
    assert!(json_stdout(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("--params"));
    // --cloud is a value enum: clap rejects it (exit 2) before main runs.
    tplc()
        .args(["--json", "api", "getDeviceList", "--cloud", "hue"])
        .assert()
        .code(2);
}

#[test]
fn schedule_add_validates_its_flags_before_any_credential() {
    for args in [
        vec!["schedule", "add", "Lamp", "--action", "on"],
        vec![
            "schedule", "add", "Lamp", "--action", "on", "--time", "25:00",
        ],
        vec![
            "schedule", "add", "Lamp", "--action", "on", "--time", "7:00", "--days", "funday",
        ],
        vec!["schedule", "edit", "Lamp", "RULE", "--time", "seven"],
        vec!["schedule", "edit", "Lamp", "RULE", "--enable", "--disable"],
    ] {
        let mut full = vec!["--json"];
        full.extend(args.iter());
        tplc().args(&full).assert().code(2);
    }
}

#[test]
fn clap_ranges_reject_out_of_range_light_values() {
    tplc()
        .args(["light", "brightness", "Strip", "150"])
        .assert()
        .code(2);
    tplc()
        .args(["light", "temp", "Strip", "1000"])
        .assert()
        .code(2);
    tplc()
        .args([
            "light",
            "color",
            "Strip",
            "--hue",
            "400",
            "--saturation",
            "10",
        ])
        .assert()
        .code(2);
    tplc()
        .args(["energy", "daily", "Plug", "--month", "13"])
        .assert()
        .code(2);
}

#[test]
fn room_writes_exit_6_when_non_interactive_without_force() {
    // Checked before any credential or network access, so it is exit 6 even
    // with no session — a driver never hangs on a prompt.
    for args in [
        vec!["rooms", "move", "Office Lamp", "--room", "Office"],
        vec!["rooms", "create", "Loft"],
        vec!["rooms", "rename", "Loft", "Attic"],
        vec!["rooms", "delete", "Loft"],
    ] {
        let mut full = vec!["--json"];
        full.extend(args.iter());
        let out = tplc().args(&full).assert().code(6);
        let v = json_stdout(&out);
        assert_eq!(v["error"]["code"], "confirmation_required", "args {args:?}");
        assert!(v["error"]["message"].as_str().unwrap().contains("--force"));
    }
}

#[test]
fn login_never_takes_the_password_on_argv() {
    // A positional password must be a clap usage error.
    tplc().args(["auth", "login", "hunter2"]).assert().code(2);
    tplc().args(["login", "hunter2"]).assert().code(2);
    // Exactly one secret source.
    tplc()
        .args([
            "--json",
            "auth",
            "login",
            "--stdin",
            "--from-env",
            "X",
            "--username",
            "user@example.com",
        ])
        .write_stdin("pw\n")
        .assert()
        .code(2);
}

#[test]
fn login_fails_on_missing_inputs_before_the_keychain() {
    // No username anywhere (flag, env, config) and non-interactive: usage.
    let out = tplc()
        .args(["--json", "auth", "login", "--non-interactive", "--stdin"])
        .write_stdin("pw\n")
        .assert()
        .code(2);
    assert!(json_stdout(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("--username"));
    // A named env var that isn't set: usage, before anything else.
    let out = tplc()
        .args([
            "--json",
            "auth",
            "login",
            "--non-interactive",
            "--from-env",
            "TPLC_TEST_UNSET_SECRET",
            "--username",
            "user@example.com",
        ])
        .assert()
        .code(2);
    assert!(json_stdout(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("TPLC_TEST_UNSET_SECRET"));
    // No password source at all, non-interactive: usage.
    let out = tplc()
        .args([
            "--json",
            "auth",
            "login",
            "--non-interactive",
            "--username",
            "user@example.com",
        ])
        .assert()
        .code(2);
    assert!(json_stdout(&out)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("--stdin"));
}

#[test]
fn set_credential_is_a_usage_error_pointing_at_login() {
    let out = tplc()
        .args(["--json", "auth", "set-credential", "--stdin"])
        .write_stdin("token\n")
        .assert()
        .code(2);
    let v = json_stdout(&out);
    assert_eq!(v["error"]["code"], "usage");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("auth login"));
}

#[test]
fn completions_render_for_zsh_and_bash() {
    tplc()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#compdef tplc"));
    tplc()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tplc"));
}

/// Gated: reads the OS keychain. Run with `TPLC_TEST_KEYCHAIN=1` on a machine
/// whose keychain holds no `tplc` session (a fresh CI box, a clean user).
#[test]
fn auth_status_works_logged_out_and_reads_exit_3() {
    if !keychain_tests_enabled() {
        eprintln!("skipped: set TPLC_TEST_KEYCHAIN=1 to run keychain-touching assertions");
        return;
    }
    let out = tplc().args(["--json", "auth", "status"]).assert().success();
    let v = json_stdout(&out);
    assert_eq!(v["schema"], "auth-status/v1");
    assert_eq!(v["required"], true);
    assert_eq!(v["authenticated"], false);
    assert_eq!(v["method"], "password");
    assert_eq!(v["credential_in_keychain"], false);

    for args in [
        vec!["devices", "list"],
        vec!["power", "status", "Lamp"],
        vec!["rooms", "list"],
        vec!["groups", "list"],
        vec!["api", "getDeviceList"],
    ] {
        let mut full = vec!["--json"];
        full.extend(args.iter());
        let out = tplc().args(&full).assert().code(3);
        let v = json_stdout(&out);
        assert_eq!(v["error"]["code"], "auth", "args {args:?}");
        assert!(v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("auth login"));
    }
}

/// The repo must carry no personal data (SPEC §1.7). Scan tracked files for
/// shapes — real-looking emails and TP-Link device identifiers — never a
/// denylist of real values. Runtime output may carry them; git may not.
#[test]
fn tracked_files_carry_no_personal_data() {
    let root = env!("CARGO_MANIFEST_DIR");
    let out = std::process::Command::new("git")
        .args(["-C", root, "ls-files"])
        .output()
        .expect("git ls-files");
    if !out.status.success() {
        eprintln!("not a git checkout; skipping");
        return;
    }
    let files = String::from_utf8_lossy(&out.stdout);
    for rel in files
        .lines()
        .filter(|f| !f.ends_with(".lock") && !f.ends_with(".pem"))
    {
        let path = format!("{root}/{rel}");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            let where_ = format!("{rel}:{}", n + 1);
            for tok in line.split(|c: char| c.is_whitespace() || "\"'`<>()[]{},;|".contains(c)) {
                if tok.contains('@') && tok.contains('.') && !tok.contains("example.com") {
                    let local = tok.split('@').next().unwrap_or("");
                    let domain = tok.rsplit('@').next().unwrap_or("");
                    let is_email = !local.is_empty()
                        && local
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
                        && domain.contains('.')
                        && domain
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || ".-".contains(c));
                    assert!(
                        !is_email,
                        "{where_}: `{tok}` looks like a real email address"
                    );
                }
                // A TP-Link device id is 40 hex chars (an outlet adds two
                // digits). Fixtures use zero-padded counters; anything else
                // that shape is a real device.
                let core = tok.trim_end_matches(|c: char| c.is_ascii_digit() && tok.len() > 40);
                if core.len() == 40 && core.chars().all(|c| c.is_ascii_hexdigit()) {
                    let dummy = core.starts_with("000000000000000000000000000000");
                    assert!(dummy, "{where_}: `{tok}` looks like a real device id");
                }
            }
        }
    }
}
