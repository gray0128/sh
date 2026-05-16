use clap::{Arg, ArgMatches, Command};
use dialoguer::{Confirm, Input};
use std::fs;
use std::path::PathBuf;

use crate::utils::{CliError, OutputFormat, OutputEnvelope, Breadcrumb};

use std::io;
use std::process::Command as SysCmd;
use std::os::unix::fs::PermissionsExt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Ensure a system user exists. If not, create a system user with no home.
fn ensure_system_user(username: &str) -> Result<(), io::Error> {
    // Check if user exists
    let status = SysCmd::new("id").arg("-u").arg(username).status();
    if let Ok(st) = status {
        if st.success() {
            return Ok(());
        }
    }
    // Create user without home (it may get a home under /home by default; use -m to create home). We'll create with a home to place authorized_keys.
    let status = SysCmd::new("useradd")
        .args(&["--create-home", "--shell", "/usr/sbin/nologin", username])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "useradd 命令执行失败"))
    }
}

/// Write authorized_keys for the given user from provided pubkey file.
fn set_authorized_keys(username: &str, pubkey_path: &std::path::Path) -> Result<(), io::Error> {
    // Read public key content
    let pubkey = fs::read_to_string(pubkey_path)?;
    // Determine user's home directory
    let output = SysCmd::new("bash")
        .arg("-c")
        .arg(format!("getent passwd {} | cut -d: -f6", username))
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, "无法获取用户主目录"));
    }
    let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if home.is_empty() {
        return Err(io::Error::new(io::ErrorKind::Other, "用户主目录为空"));
    }
    let ssh_dir = std::path::Path::new(&home).join(".ssh");
    fs::create_dir_all(&ssh_dir)?;
    // Set permissions 700 on .ssh
    fs::set_permissions(&ssh_dir, fs::Permissions::from_mode(0o700))?;
    let auth_file = ssh_dir.join("authorized_keys");
    fs::write(&auth_file, pubkey)?;
    fs::set_permissions(&auth_file, fs::Permissions::from_mode(0o600))?;
    // Change owner to the user: use chown command
    let status = SysCmd::new("chown")
        .args(&["-R", &format!("{}:{}", username, username), ssh_dir.to_str().unwrap()])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "chown 操作失败"))
    }
}

/// Write a sshd configuration snippet to /etc/ssh/sshd_config.d/vps-cli.conf
fn write_ssh_snippet(port: Option<&String>, user: Option<&String>) -> Result<(), io::Error> {
    // Build configuration lines
    let mut lines = Vec::new();
    lines.push("# vps-cli managed configuration".to_string());
    if let Some(p) = port {
        lines.push(format!("Port {}", p));
    }
    lines.push("PermitRootLogin no".to_string());
    lines.push("PasswordAuthentication no".to_string());
    lines.push("KbdInteractiveAuthentication no".to_string());
    lines.push("ChallengeResponseAuthentication no".to_string());
    if let Some(u) = user {
        lines.push(format!("AllowUsers {}", u));
    }
    let content = lines.join("\n") + "\n";

    // Ensure include directive in main sshd_config
    ensure_include_directive()?;

    // Write snippet with backup
    fs::create_dir_all("/etc/ssh/sshd_config.d")?;
    let path = std::path::Path::new("/etc/ssh/sshd_config.d/vps-cli.conf");
    // Backup existing file
    if path.exists() {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let backup_path = path.with_extension(format!("bak.{}", ts));
        fs::copy(path, &backup_path)?;
    }
    fs::write(path, content)?;
    Ok(())
}

/// Test sshd configuration and reload service
fn test_and_reload_sshd() -> Result<(), io::Error> {
    // Test config
    let status = SysCmd::new("sshd").args(&["-t"]).status();
    if let Ok(st) = status {
        if !st.success() {
            return Err(io::Error::new(io::ErrorKind::Other, "sshd 配置测试失败"));
        }
    }
    // Reload via systemctl; try sshd and ssh
    let candidates = vec!["sshd", "ssh"];
    for service in candidates {
        let st = SysCmd::new("systemctl").args(&["reload", service]).status();
        if let Ok(status) = st {
            if status.success() {
                return Ok(());
            }
        }
    }
    Err(io::Error::new(io::ErrorKind::Other, "无法重载 SSH 服务"))
}

/// Check if the given port is already listening on TCP.
fn is_port_in_use(port: u16) -> bool {
    // Use ss or netstat to check listening ports
    // Try ss
    let cmd = SysCmd::new("sh").arg("-c").arg(format!("ss -tuln | grep -w ':{}'", port)).output();
    if let Ok(output) = cmd {
        if !output.stdout.is_empty() { return true; }
    }
    // Fallback to netstat
    let cmd2 = SysCmd::new("sh").arg("-c").arg(format!("netstat -tuln | grep -w ':{}'", port)).output();
    if let Ok(output) = cmd2 {
        if !output.stdout.is_empty() { return true; }
    }
    false
}

/// Ensure sshd_config includes /etc/ssh/sshd_config.d/* if not already present.
fn ensure_include_directive() -> Result<(), io::Error> {
    let config_path = "/etc/ssh/sshd_config";
    let contents = fs::read_to_string(config_path)?;
    // Look for Include directive pointing to sshd_config.d
    let has_include = contents.lines().any(|l| {
        let ltrim = l.trim_start();
        ltrim.starts_with("Include") && ltrim.contains("sshd_config.d")
    });
    if !has_include {
        // Backup original
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let backup_path = format!("{}{}.bak.{}", config_path, "", ts);
        fs::copy(config_path, &backup_path)?;
        // Append include directive
        let mut new_content = contents;
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str("Include /etc/ssh/sshd_config.d/*.conf\n");
        fs::write(config_path, new_content)?;
    }
    Ok(())
}

/// Configure firewall to allow the SSH port. Detects ufw or firewalld.
fn configure_firewall(port: u16) -> Result<(), io::Error> {
    // Check for ufw
    let ufw_exists = SysCmd::new("which").arg("ufw").output().map(|o| o.status.success()).unwrap_or(false);
    if ufw_exists {
        // enable ufw if not enabled (but not messing existing state). We just allow port
        let status = SysCmd::new("ufw").args(&["allow", &format!("{}/tcp", port)]).status()?;
        if status.success() {
            return Ok(());
        }
    }
    // Check for firewalld
    let fw_exists = SysCmd::new("which").arg("firewall-cmd").output().map(|o| o.status.success()).unwrap_or(false);
    if fw_exists {
        let status1 = SysCmd::new("firewall-cmd").args(&["--permanent", &format!("--add-port={}/tcp", port)]).status()?;
        // Some systems require reload afterwards
        let status2 = SysCmd::new("firewall-cmd").arg("--reload").status()?;
        if status1.success() && status2.success() {
            return Ok(());
        }
    }
    // If no firewall tool present, we silently skip
    Ok(())
}

pub fn cli() -> Command {
    Command::new("setup-ssh")
        .about("配置并加固 SSH 服务（仅限 Ubuntu）")
        .arg(
            Arg::new("port")
                .long("port")
                .value_name("PORT")
                .help("新的 SSH 端口，范围 1024-65535"),
        )
        .arg(
            Arg::new("user")
                .long("user")
                .value_name("USERNAME")
                .help("要允许登录的普通用户名"),
        )
        .arg(
            Arg::new("pubkey-file")
                .long("pubkey-file")
                .value_name("FILE")
                .help("包含公钥的文件路径"),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .help("预览配置更改，但不实际修改文件或重启服务")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("confirm")
                .long("confirm")
                .help("跳过交互确认，直接应用更改")
                .action(clap::ArgAction::SetTrue),
        )
}

pub fn handle_setup_ssh(matches: &ArgMatches, format: OutputFormat, no_input: bool) -> Result<(), CliError> {
    // Root and OS check
    if !nix::unistd::Uid::effective().is_root() {
        return Err(CliError::new("此命令必须以 root 权限运行"));
    }
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    if !os_release.contains("Ubuntu") {
        return Err(CliError::new("当前系统不是 Ubuntu，脚本仅在 Ubuntu 上测试过"));
    }

    let dry_run = matches.get_flag("dry-run");
    let confirm_flag = matches.get_flag("confirm");

    // Parse port
    let mut port = matches.get_one::<String>("port").cloned();
    let mut user = matches.get_one::<String>("user").cloned();
    let mut pubkey_path = matches.get_one::<String>("pubkey-file").map(PathBuf::from);

    // interactive prompts if missing
    let interactive = atty::is(atty::Stream::Stdin) && !no_input;
    if interactive {
        // Prompt port if missing
        if port.is_none() {
            let input: String = Input::new()
                .with_prompt("请输入新的 SSH 端口 (1024-65535)，留空则保持现有端口")
                .allow_empty(true)
                .interact_text()
                .map_err(|e| CliError::new(format!("读取端口失败: {}", e)))?;
            if !input.trim().is_empty() {
                port = Some(input);
            }
        }
        if user.is_none() {
            let input: String = Input::new()
                .with_prompt("请输入允许登录的用户名 (可留空以仅使用公钥)")
                .allow_empty(true)
                .interact_text()
                .map_err(|e| CliError::new(format!("读取用户名失败: {}", e)))?;
            if !input.trim().is_empty() {
                user = Some(input);
            }
        }
        if pubkey_path.is_none() {
            let input: String = Input::new()
                .with_prompt("请输入公钥文件路径 (留空表示使用现有 authorized_keys)")
                .allow_empty(true)
                .interact_text()
                .map_err(|e| CliError::new(format!("读取公钥路径失败: {}", e)))?;
            if !input.trim().is_empty() {
                pubkey_path = Some(PathBuf::from(input));
            }
        }
    } else {
        // non-interactive mode: error if required parameters missing
        if port.is_none() && user.is_none() && pubkey_path.is_none() {
            return Err(CliError::new("非交互模式下，必须至少提供 --port、--user 或 --pubkey-file 中的一个"));
        }
    }

    // Validate port
    if let Some(ref p) = port {
        let port_num: u16 = p.parse().map_err(|_| CliError::new("端口必须是数字"))?;
        if port_num < 1024 || port_num > 65535 {
            return Err(CliError::new("端口必须在 1024-65535 之间"));
        }
        // Check if port is already in use
        if is_port_in_use(port_num) {
            return Err(CliError::new(format!("端口 {} 已被其他服务占用", port_num)));
        }
    }

    // Validate user (only alphanumeric and underscore)
    if let Some(ref u) = user {
        if !u.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            return Err(CliError::new("用户名只能包含字母、数字、下划线或短横线"));
        }
    }

    // Validate pubkey file exists
    if let Some(ref path) = pubkey_path {
        if !path.exists() {
            return Err(CliError::new(format!("公钥文件不存在: {}", path.display())));
        }
    }

    // Determine whether to continue
    if interactive && !confirm_flag && !dry_run {
        let proceed = Confirm::new()
            .with_prompt("确定要应用这些更改吗？")
            .default(true)
            .interact()
            .map_err(|e| CliError::new(format!("读取确认输入失败: {}", e)))?;
        if !proceed {
            return Ok(());
        }
    } else if !interactive && !confirm_flag && !dry_run {
        return Err(CliError::new("非交互模式下必须使用 --confirm 标志以应用更改"));
    }

    // Simulate modifications: read current config, update accordingly
    if dry_run {
        match format {
            OutputFormat::Json => {
                let out: OutputEnvelope<serde_json::Value> = OutputEnvelope {
                    ok: true,
                    data: Some(serde_json::json!({"dry_run": true, "port": port, "user": user})),
                    error: None,
                    breadcrumbs: Some(vec![Breadcrumb { action: "apply".into(), cmd: "vps-cli setup-ssh --confirm ...".into() }]),
                };
                println!("{}", serde_json::to_string(&out).unwrap());
            }
            _ => {
                println!("[Dry Run] 将应用以下配置:");
                if let Some(ref p) = port {
                    println!("  - SSH 端口: {}", p);
                }
                if let Some(ref u) = user {
                    println!("  - 允许用户: {}", u);
                }
                if let Some(ref pk) = pubkey_path {
                    println!("  - 公钥文件: {}", pk.display());
                }
                println!("要真正应用更改，请加上 --confirm 标志");
            }
        }
        return Ok(());
    }

    // Perform actual configuration
    // Ensure user exists if provided
    if let Some(ref u) = user {
        ensure_system_user(u).map_err(|e| CliError::new(format!("创建或检查用户失败: {}", e)))?;
    }
    // If a pubkey is supplied, ensure we have a user to install into
    if pubkey_path.is_some() && user.is_none() {
        return Err(CliError::new("使用 --pubkey-file 时必须同时指定 --user"));
    }
    if let Some(ref path) = pubkey_path {
        let u = user.as_ref().unwrap();
        set_authorized_keys(u, path).map_err(|e| CliError::new(format!("写入 authorized_keys 失败: {}", e)))?;
    }
    // Write sshd snippet
    write_ssh_snippet(port.as_ref(), user.as_ref()).map_err(|e| CliError::new(format!("写入 SSH 配置失败: {}", e)))?;
    // Test and reload sshd
    test_and_reload_sshd().map_err(|e| CliError::new(format!("重载 SSH 服务失败: {}", e)))?;
    // Configure firewall if port specified
    if let Some(ref p) = port {
        let port_num: u16 = p.parse().unwrap();
        configure_firewall(port_num).map_err(|e| CliError::new(format!("配置防火墙失败: {}", e)))?;
    }
    // Output success
    match format {
        OutputFormat::Json => {
            let out: OutputEnvelope<serde_json::Value> = OutputEnvelope {
                ok: true,
                data: Some(serde_json::json!({"port": port, "user": user})),
                error: None,
                breadcrumbs: Some(vec![Breadcrumb { action: "verify", cmd: "vps-cli setup-ssh --status --json".into() }]),
            };
            println!("{}", serde_json::to_string(&out).unwrap());
        }
        _ => {
            println!("已成功应用 SSH 配置。");
            if let Some(ref p) = port {
                println!("新的端口: {}", p);
            }
            if let Some(ref u) = user {
                println!("允许用户: {}", u);
            }
        }
    }
    Ok(())
}
