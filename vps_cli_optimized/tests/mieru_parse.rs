use std::process::Command;

#[test]
fn mieru_help_includes_expected_subcommands() {
    let output = Command::new(env!("CARGO_BIN_EXE_vps-cli"))
        .args(["mieru", "--help"])
        .output()
        .expect("运行 mieru --help 失败");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("install"));
    assert!(stdout.contains("add-node"));
    assert!(stdout.contains("list-nodes"));
    assert!(stdout.contains("show-links"));
    assert!(stdout.contains("show-simple-links"));
    assert!(stdout.contains("show-standard-links"));
    assert!(stdout.contains("show-config"));
    assert!(stdout.contains("status"));
}
