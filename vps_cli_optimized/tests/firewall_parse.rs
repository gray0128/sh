use std::process::Command;

#[test]
fn firewall_help_includes_ports_query() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["firewall", "--help"])
        .output()
        .expect("运行 firewall --help 失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ports"));
    assert!(stdout.contains("查看防火墙开放端口与协议"));
}

#[test]
fn firewall_ports_help_includes_port_filter() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["firewall", "ports", "--help"])
        .output()
        .expect("运行 firewall ports --help 失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--port"));
    assert!(stdout.contains("仅查看指定端口的规则"));
}
