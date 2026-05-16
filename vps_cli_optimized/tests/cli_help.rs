use std::process::Command;

#[test]
fn top_level_help_contains_new_command_groups() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .arg("--help")
        .output()
        .expect("运行 --help 失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("setup-ssh"));
    assert!(stdout.contains("singbox"));
    assert!(stdout.contains("mieru"));
    assert!(stdout.contains("reclaim"));
    assert!(stdout.contains("管理 VPS SSH 和 sing-box 的工具"));
}
