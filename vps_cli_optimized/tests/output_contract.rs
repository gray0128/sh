use serde_json::Value;
use std::process::Command;

#[test]
fn json_failure_contract_is_stable() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["--json", "--no-input", "singbox", "install"])
        .output()
        .expect("运行命令失败");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("stdout 不是 JSON");
    assert_eq!(payload["ok"], false);
    let err = payload["error"].as_str().unwrap_or_default();
    assert!(err.contains("--confirm") || err.contains("确认"));
}

#[test]
fn json_dry_run_success_contract_is_stable() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["--json", "singbox", "install", "--dry-run"])
        .output()
        .expect("运行命令失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("stdout 不是 JSON");
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["data"]["dry_run"], true);
}

#[test]
fn trusttunnel_install_dry_run_json_contract_is_stable() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["--json", "trusttunnel", "install", "--dry-run"])
        .output()
        .expect("运行命令失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("stdout 不是 JSON");
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["data"]["dry_run"], true);
    assert_eq!(payload["data"]["output_dir"], "/opt/trusttunnel");
}

#[test]
fn reclaim_cleanup_requires_confirm_in_json_mode() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["--json", "--no-input", "reclaim", "singbox-purge"])
        .output()
        .expect("运行命令失败");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("stdout 不是 JSON");
    assert_eq!(payload["ok"], false);
    let err = payload["error"].as_str().unwrap_or_default();
    assert!(err.contains("--confirm") || err.contains("确认"));
}

#[test]
fn trusttunnel_purge_requires_confirm_in_json_mode() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["--json", "--no-input", "trusttunnel", "purge"])
        .output()
        .expect("运行命令失败");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("stdout 不是 JSON");
    assert_eq!(payload["ok"], false);
    let err = payload["error"].as_str().unwrap_or_default();
    assert!(err.contains("--confirm") || err.contains("确认"));
}

#[test]
fn version_json_contains_current_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["--json", "version"])
        .output()
        .expect("运行命令失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("stdout 不是 JSON");
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["data"]["current_version"], "0.2.0");
    assert_eq!(payload["data"]["latest_release_version"], "0.2.0");
}

#[test]
fn trusttunnel_export_config_hides_sensitive_output_without_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args([
            "--json",
            "trusttunnel",
            "export-config",
            "--client",
            "demo",
            "--address",
            "example.com:443",
        ])
        .output()
        .expect("运行命令失败");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("stdout 不是 JSON");
    assert_eq!(payload["ok"], false);
    let err = payload["error"].as_str().unwrap_or_default();
    assert!(err.contains("trusttunnel_endpoint") || err.contains("install"));
}

#[test]
fn mieru_add_node_show_secrets_only_returns_follow_up_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args([
            "--json",
            "--no-input",
            "mieru",
            "add-node",
            "--host",
            "example.com",
            "--port",
            "8443",
            "--protocol",
            "TCP",
            "--dry-run",
            "--confirm",
            "--show-secrets",
        ])
        .output()
        .expect("运行命令失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("stdout 不是 JSON");
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["data"]["sensitive_output_split"], true);
    assert!(payload["data"]["simple_link"].is_null());
    assert!(payload["data"]["client_json"].is_null());
    assert_eq!(
        payload["data"]["next_steps"]["show_simple_links"],
        "vps-cli mieru show-simple-links --show-secrets"
    );
}
