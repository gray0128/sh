use std::process::Command;

#[test]
fn setup_ssh_help_includes_security_flags() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps_cli"))
        .args(["setup-ssh", "--help"])
        .output()
        .expect("运行 setup-ssh --help 失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--set-allow-users"));
    assert!(stdout.contains("--disable-root-login"));
    assert!(stdout.contains("--disable-password-auth"));
    assert!(stdout.contains("--write-hardening"));
    assert!(stdout.contains("--rotate-authorized-key"));
    assert!(stdout.contains("--skip-pubkey"));
}
