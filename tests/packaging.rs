//! The Claude Desktop extension manifest (`packaging/mcpb/manifest.json`)
//! against the binary it packages, so a release cannot ship a manifest that
//! names a tool or a version the binary does not have, or one that asks for
//! a credential. The release workflow validates the schema with the `mcpb`
//! CLI.

use std::process::Command;

use serde_json::{json, Value};

fn manifest() -> Value {
    let path = format!(
        "{}/packaging/mcpb/manifest.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_manifest_version_is_the_crate_version() {
    assert_eq!(manifest()["version"], env!("CARGO_PKG_VERSION"));
}

/// AGENTS.md names the spec's tag; a pin bump that forgets it leaves agents
/// reading the wrong spec.
#[test]
fn agents_md_names_the_pinned_cli_common_tag() {
    let root = env!("CARGO_MANIFEST_DIR");
    let cargo = std::fs::read_to_string(format!("{root}/Cargo.toml")).unwrap();
    let tag = cargo
        .lines()
        .find(|l| l.starts_with("pk-cli-core"))
        .and_then(|l| l.split("tag = \"").nth(1))
        .and_then(|rest| rest.split('"').next())
        .expect("pk-cli-core is pinned to a tag");
    let agents = std::fs::read_to_string(format!("{root}/AGENTS.md")).unwrap();
    assert!(
        agents.contains(&format!("**{tag}**")),
        "AGENTS.md does not name {tag}"
    );
}

#[test]
fn the_manifest_lists_exactly_the_served_tools() {
    let listed: Vec<String> = manifest()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    let out = Command::new(env!("CARGO_BIN_EXE_tplc"))
        .args(["mcp", "--list-tools"])
        .env("XDG_CONFIG_HOME", env!("CARGO_TARGET_TMPDIR"))
        .env("HOME", env!("CARGO_TARGET_TMPDIR"))
        .env("USERPROFILE", env!("CARGO_TARGET_TMPDIR"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let served: Vec<String> = serde_json::from_slice::<Value>(&out.stdout).unwrap()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(listed, served);
}

/// SECURITY.md says why there is no Windows build.
#[test]
fn the_extension_does_not_claim_windows() {
    let m = manifest();
    assert_eq!(m["compatibility"]["platforms"], json!(["darwin", "linux"]));
    assert!(m["server"]["mcp_config"]["platform_overrides"]
        .get("win32")
        .is_none());
}

/// A login can ask for an emailed code, which a settings form cannot do, so the extension asks
/// for nothing and passes nothing: the binary reads the session `tplc auth login` stored in the
/// OS keychain, and renews it there.
#[test]
fn every_platform_runs_mcp_with_no_credential_in_its_config() {
    let m = manifest();
    assert!(m.get("user_config").is_none(), "no settings to fill in");
    let config = &m["server"]["mcp_config"];
    let mut configs = vec![config.clone()];
    configs.push(config["platform_overrides"]["linux"].clone());
    for c in &configs {
        assert_eq!(c["args"], json!(["mcp"]), "{c}");
        assert!(
            c["command"]
                .as_str()
                .unwrap()
                .starts_with("${__dirname}/server/"),
            "{c}"
        );
        assert!(c.get("env").is_none(), "{c}");
    }
    let text = m["long_description"].as_str().unwrap();
    assert!(text.contains("tplc auth login"), "{text}");
    assert!(text.contains("goes to Claude"), "{text}");
    assert!(text.contains("asks you"), "{text}");
    assert_eq!(m["license"], "GPL-3.0");
}
