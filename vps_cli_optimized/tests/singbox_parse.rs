use std::process::Command;

#[test]
fn singbox_help_includes_protocol_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["singbox", "--help"])
        .output()
        .expect("运行 singbox --help 失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("add-vless-reality"));
    assert!(stdout.contains("add-trojan-tls"));
    assert!(stdout.contains("add-hysteria2-tls"));
    assert!(stdout.contains("add-tuic-tls"));
    assert!(stdout.contains("add-shadowsocks"));
    assert!(stdout.contains("show-links"));
    assert!(stdout.contains("check-config"));
    assert!(stdout.contains("logs"));
}
