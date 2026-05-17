use std::process::Command;

#[test]
fn port_help_includes_usage_query() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["port", "--help"])
        .output()
        .expect("运行 port --help 失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("usage"));
    assert!(stdout.contains("查看端口占用与监听进程"));
}

#[test]
fn port_usage_help_includes_port_filter_and_all_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["port", "usage", "--help"])
        .output()
        .expect("运行 port usage --help 失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--port"));
    assert!(stdout.contains("--all"));
    assert!(stdout.contains("仅查看指定端口"));
    assert!(stdout.contains("包含非监听连接"));
}
