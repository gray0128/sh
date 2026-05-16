use std::process::Command;

#[test]
fn reclaim_help_includes_dangerous_subcommands() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps_cli"))
        .args(["reclaim", "--help"])
        .output()
        .expect("运行 reclaim --help 失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("singbox-uninstall"));
    assert!(stdout.contains("singbox-purge"));
    assert!(stdout.contains("audit-proxies"));
    assert!(stdout.contains("cleanup-proxies"));
    assert!(stdout.contains("audit-nginx"));
    assert!(stdout.contains("cleanup-nginx"));
    assert!(stdout.contains("audit-caddy"));
    assert!(stdout.contains("cleanup-caddy"));
    assert!(stdout.contains("mieru-uninstall"));
}
