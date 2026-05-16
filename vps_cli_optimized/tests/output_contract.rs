use serde_json::Value;
use std::process::Command;

#[test]
fn json_failure_contract_is_stable() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps_cli"))
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
    let output = Command::new(env!("CARGO_BIN_EXE_vps_cli"))
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
fn reclaim_cleanup_requires_confirm_in_json_mode() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps_cli"))
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
