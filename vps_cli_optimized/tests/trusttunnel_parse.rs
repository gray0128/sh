use std::process::Command;

#[test]
fn trusttunnel_help_includes_expected_subcommands() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["trusttunnel", "--help"])
        .output()
        .expect("运行 trusttunnel --help 失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("install"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("start"));
    assert!(stdout.contains("stop"));
    assert!(stdout.contains("restart"));
    assert!(stdout.contains("logs"));
    assert!(stdout.contains("setup-wizard"));
    assert!(stdout.contains("export-config"));
    assert!(stdout.contains("uninstall"));
    assert!(stdout.contains("purge"));
}
