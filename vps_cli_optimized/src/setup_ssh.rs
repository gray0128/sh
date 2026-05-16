use clap::{Arg, ArgAction, ArgMatches, Command};
use dialoguer::{Confirm, Input};
use serde_json::{json, Value as JsonValue};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as SysCmd;

use crate::safety::{
    append_backup, backup_path, ensure_dir, is_interactive, require_confirmation, restore_backup,
};
use crate::utils::{Breadcrumb, CliError, OperationReport, OutputEnvelope, OutputFormat};

const SSH_BACKUP_DIR: &str = "/root/ssh-setup-backups";
const SSHD_CONFIG: &str = "/etc/ssh/sshd_config";
const MANAGED_CONFIG: &str = "/etc/ssh/sshd_config.d/vps-cli.conf";

#[derive(Debug, Default)]
struct SetupPlan {
    port: Option<u16>,
    user: Option<String>,
    pubkey_file: Option<PathBuf>,
    rotate_authorized_key: bool,
    skip_pubkey: bool,
    allow_users: Option<String>,
    disable_root_login: bool,
    disable_password_auth: bool,
    write_hardening: bool,
}

#[derive(Debug, Default)]
struct ApplyResult {
    report: OperationReport,
    port: Option<u16>,
    allow_users: Option<String>,
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
                .help("要配置公钥或纳入白名单的用户名"),
        )
        .arg(
            Arg::new("pubkey-file")
                .long("pubkey-file")
                .value_name("FILE")
                .help("新的 SSH 公钥文件路径"),
        )
        .arg(
            Arg::new("set-allow-users")
                .long("set-allow-users")
                .value_name("USERS")
                .help("显式写入 AllowUsers 列表，空格分隔"),
        )
        .arg(
            Arg::new("disable-root-login")
                .long("disable-root-login")
                .action(ArgAction::SetTrue)
                .help("写入 PermitRootLogin no"),
        )
        .arg(
            Arg::new("disable-password-auth")
                .long("disable-password-auth")
                .action(ArgAction::SetTrue)
                .help("写入 PasswordAuthentication no 及相关键盘交互配置"),
        )
        .arg(
            Arg::new("write-hardening")
                .long("write-hardening")
                .action(ArgAction::SetTrue)
                .help("写入基础 SSH 加固项"),
        )
        .arg(
            Arg::new("rotate-authorized-key")
                .long("rotate-authorized-key")
                .action(ArgAction::SetTrue)
                .help("替换目标用户的 authorized_keys，仅保留这一个新公钥"),
        )
        .arg(
            Arg::new("skip-pubkey")
                .long("skip-pubkey")
                .action(ArgAction::SetTrue)
                .help("跳过公钥替换流程"),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .action(ArgAction::SetTrue)
                .help("仅预览变更，不实际写入"),
        )
        .arg(
            Arg::new("confirm")
                .long("confirm")
                .action(ArgAction::SetTrue)
                .help("跳过交互确认，直接应用"),
        )
}

pub fn handle_setup_ssh(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    ensure_root_and_ubuntu()?;
    let dry_run = matches.get_flag("dry-run");
    let confirm = matches.get_flag("confirm");
    let interactive = is_interactive(no_input);

    let mut plan = SetupPlan {
        port: matches
            .get_one::<String>("port")
            .cloned()
            .map(parse_port)
            .transpose()?,
        user: matches.get_one::<String>("user").cloned(),
        pubkey_file: matches.get_one::<String>("pubkey-file").map(PathBuf::from),
        rotate_authorized_key: matches.get_flag("rotate-authorized-key"),
        skip_pubkey: matches.get_flag("skip-pubkey"),
        allow_users: matches.get_one::<String>("set-allow-users").cloned(),
        disable_root_login: matches.get_flag("disable-root-login"),
        disable_password_auth: matches.get_flag("disable-password-auth"),
        write_hardening: matches.get_flag("write-hardening"),
    };

    fill_plan_interactively(&mut plan, interactive)?;
    validate_plan(&plan)?;
    require_confirmation(
        "将对 SSH 配置执行高风险修改，确认继续？",
        interactive,
        dry_run,
        confirm,
    )?;

    if dry_run {
        emit_success(
            format,
            json!({
                "dry_run": true,
                "port": plan.port,
                "user": plan.user,
                "allow_users": plan.allow_users,
                "rotate_authorized_key": plan.rotate_authorized_key,
                "disable_root_login": plan.disable_root_login,
                "disable_password_auth": plan.disable_password_auth,
                "write_hardening": plan.write_hardening
            }),
            &OperationReport::default(),
            Some(vec![crumb("执行修改", "vps-cli setup-ssh --confirm ...")]),
        );
        return Ok(());
    }

    let result = apply_plan(&plan)?;
    emit_success(
        format,
        json!({
            "applied": true,
            "port": result.port,
            "allow_users": result.allow_users,
        }),
        &result.report,
        Some(vec![crumb(
            "检查 SSH 状态",
            "sudo sshd -t && sudo sshd -T | head",
        )]),
    );
    Ok(())
}

fn ensure_root_and_ubuntu() -> Result<(), CliError> {
    if !nix::unistd::Uid::effective().is_root() {
        return Err(CliError::new("此命令必须以 root 权限运行"));
    }
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    if !os_release.contains("Ubuntu") {
        return Err(CliError::new(
            "当前系统不是 Ubuntu，脚本仅在 Ubuntu 上测试过",
        ));
    }
    Ok(())
}

fn fill_plan_interactively(plan: &mut SetupPlan, interactive: bool) -> Result<(), CliError> {
    if !interactive {
        return Ok(());
    }
    if plan.port.is_none() {
        let input: String = Input::new()
            .with_prompt("请输入新的 SSH 端口（留空表示不修改）")
            .allow_empty(true)
            .interact_text()
            .map_err(|e| CliError::new(format!("读取端口失败: {}", e)))?;
        if !input.trim().is_empty() {
            plan.port = Some(parse_port(input)?);
        }
    }
    if plan.user.is_none() {
        let input: String = Input::new()
            .with_prompt("请输入需要管理的 Linux 用户（留空表示不涉及公钥/白名单）")
            .allow_empty(true)
            .interact_text()
            .map_err(|e| CliError::new(format!("读取用户失败: {}", e)))?;
        if !input.trim().is_empty() {
            plan.user = Some(input);
        }
    }
    if plan.pubkey_file.is_none() && !plan.skip_pubkey {
        let want_key = Confirm::new()
            .with_prompt("是否替换目标用户的 authorized_keys？")
            .default(false)
            .interact()
            .map_err(|e| CliError::new(format!("读取确认失败: {}", e)))?;
        if want_key {
            let input: String = Input::new()
                .with_prompt("请输入新的 SSH 公钥文件路径")
                .interact_text()
                .map_err(|e| CliError::new(format!("读取公钥路径失败: {}", e)))?;
            plan.pubkey_file = Some(PathBuf::from(input));
            plan.rotate_authorized_key = true;
        } else {
            plan.skip_pubkey = true;
        }
    }
    if plan.allow_users.is_none() {
        let set_allow_users = Confirm::new()
            .with_prompt("是否显式写入 AllowUsers 白名单？")
            .default(false)
            .interact()
            .map_err(|e| CliError::new(format!("读取确认失败: {}", e)))?;
        if set_allow_users {
            let default = plan.user.clone().unwrap_or_default();
            let input: String = Input::new()
                .with_prompt("请输入 AllowUsers 内容（空格分隔）")
                .with_initial_text(default)
                .interact_text()
                .map_err(|e| CliError::new(format!("读取 AllowUsers 失败: {}", e)))?;
            if !input.trim().is_empty() {
                plan.allow_users = Some(input);
            }
        }
    }
    if !plan.disable_root_login {
        plan.disable_root_login = Confirm::new()
            .with_prompt("是否禁止 root SSH 登录？")
            .default(false)
            .interact()
            .map_err(|e| CliError::new(format!("读取确认失败: {}", e)))?;
    }
    if !plan.disable_password_auth {
        plan.disable_password_auth = Confirm::new()
            .with_prompt("是否禁用全局 SSH 密码认证？")
            .default(false)
            .interact()
            .map_err(|e| CliError::new(format!("读取确认失败: {}", e)))?;
    }
    if !plan.write_hardening {
        plan.write_hardening = Confirm::new()
            .with_prompt("是否写入基础 SSH 加固项？")
            .default(true)
            .interact()
            .map_err(|e| CliError::new(format!("读取确认失败: {}", e)))?;
    }
    Ok(())
}

fn validate_plan(plan: &SetupPlan) -> Result<(), CliError> {
    if let Some(port) = plan.port {
        if is_port_in_use(port) {
            return Err(CliError::new(format!("端口 {} 已被其他服务占用", port)));
        }
    }
    if let Some(ref user) = plan.user {
        if !user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(CliError::new("用户名只能包含字母、数字、下划线或短横线"));
        }
    }
    if let Some(ref path) = plan.pubkey_file {
        if !path.exists() {
            return Err(CliError::new(format!("公钥文件不存在: {}", path.display())));
        }
        validate_public_key(path)?;
    }
    if plan.rotate_authorized_key && plan.pubkey_file.is_none() {
        return Err(CliError::new(
            "启用 --rotate-authorized-key 时必须同时提供 --pubkey-file",
        ));
    }
    if plan.pubkey_file.is_some() && plan.user.is_none() {
        return Err(CliError::new("指定 --pubkey-file 时必须同时提供 --user"));
    }
    if let (Some(allow_users), Some(user)) = (&plan.allow_users, &plan.user) {
        if plan.rotate_authorized_key && !allow_users.split_whitespace().any(|item| item == user) {
            return Err(CliError::new(format!(
                "AllowUsers 必须包含本次替换公钥的用户 {}",
                user
            )));
        }
    }
    Ok(())
}

fn apply_plan(plan: &SetupPlan) -> Result<ApplyResult, CliError> {
    ensure_dir(Path::new(SSH_BACKUP_DIR))?;
    let main_backup = backup_path(
        Path::new(SSHD_CONFIG),
        Path::new(SSH_BACKUP_DIR),
        "sshd_config",
    )?;
    let managed_backup = backup_path(
        Path::new(MANAGED_CONFIG),
        Path::new(SSH_BACKUP_DIR),
        "managed",
    )?;

    if let Some(ref user) = plan.user {
        ensure_system_user(user)?;
    }

    ensure_include_directive()?;
    let mut report = OperationReport::default();
    append_backup(&mut report, &main_backup);
    append_backup(&mut report, &managed_backup);

    let mut auth_backup = None;
    let mut auth_target = None;
    if let (Some(path), Some(user)) = (&plan.pubkey_file, &plan.user) {
        let (target, backup) = replace_authorized_keys(user, path, plan.rotate_authorized_key)?;
        auth_target = Some(target);
        auth_backup = backup;
        append_backup(&mut report, &auth_backup);
    }

    let content = render_managed_config(plan)?;
    fs::create_dir_all("/etc/ssh/sshd_config.d")
        .map_err(|e| CliError::new(format!("创建 sshd_config.d 失败: {}", e)))?;
    fs::write(MANAGED_CONFIG, &content)
        .map_err(|e| CliError::new(format!("写入托管配置失败: {}", e)))?;
    report.changed_files.push(MANAGED_CONFIG.into());
    if let Some(ref path) = plan.pubkey_file {
        report.changed_files.push(path.display().to_string());
    }

    let apply_result = validate_and_reload(plan);
    if let Err(err) = apply_result {
        restore_all(
            &main_backup,
            &managed_backup,
            &auth_backup,
            auth_target.as_deref(),
        )?;
        report.rolled_back = Some(true);
        report
            .warnings
            .push("校验或重载失败，已尝试自动恢复 SSH 配置。".into());
        return Err(err
            .with_warnings(vec!["校验或重载失败，已尝试自动恢复 SSH 配置。".into()])
            .with_report(&report));
    }
    if let Some(port) = plan.port {
        configure_firewall(port)?;
    }
    report.rolled_back = Some(false);

    Ok(ApplyResult {
        report,
        port: plan.port,
        allow_users: plan.allow_users.clone(),
    })
}

fn ensure_system_user(username: &str) -> Result<(), CliError> {
    let status = SysCmd::new("id").arg("-u").arg(username).status();
    if let Ok(st) = status {
        if st.success() {
            return Ok(());
        }
    }
    let status = SysCmd::new("useradd")
        .args(["--create-home", "--shell", "/usr/sbin/nologin", username])
        .status()
        .map_err(|e| CliError::new(format!("执行 useradd 失败: {}", e)))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::new(format!("创建用户失败: {}", username)))
    }
}

fn ensure_include_directive() -> Result<(), CliError> {
    let contents = fs::read_to_string(SSHD_CONFIG)
        .map_err(|e| CliError::new(format!("读取 sshd_config 失败: {}", e)))?;
    let has_include = contents.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("Include") && trimmed.contains("sshd_config.d")
    });
    if has_include {
        return Ok(());
    }
    let mut new_content = contents;
    if !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str("Include /etc/ssh/sshd_config.d/*.conf\n");
    fs::write(SSHD_CONFIG, new_content)
        .map_err(|e| CliError::new(format!("写入 sshd_config 失败: {}", e)))
}

fn replace_authorized_keys(
    username: &str,
    pubkey_path: &Path,
    replace_existing: bool,
) -> Result<(PathBuf, Option<PathBuf>), CliError> {
    let pubkey = fs::read_to_string(pubkey_path)
        .map_err(|e| CliError::new(format!("读取公钥文件失败: {}", e)))?;
    let home = user_home(username)?;
    let ssh_dir = home.join(".ssh");
    fs::create_dir_all(&ssh_dir)
        .map_err(|e| CliError::new(format!("创建 .ssh 目录失败: {}", e)))?;
    fs::set_permissions(&ssh_dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| CliError::new(format!("设置 .ssh 权限失败: {}", e)))?;
    let auth_file = ssh_dir.join("authorized_keys");
    let backup = backup_path(
        &auth_file,
        Path::new(SSH_BACKUP_DIR),
        &format!("authorized_keys.{}", username),
    )?;
    let final_content = if replace_existing || !auth_file.exists() {
        pubkey.clone()
    } else {
        let mut existing = fs::read_to_string(&auth_file).unwrap_or_default();
        if !existing.ends_with('\n') && !existing.is_empty() {
            existing.push('\n');
        }
        existing.push_str(&pubkey);
        existing
    };
    fs::write(&auth_file, final_content)
        .map_err(|e| CliError::new(format!("写入 authorized_keys 失败: {}", e)))?;
    fs::set_permissions(&auth_file, fs::Permissions::from_mode(0o600))
        .map_err(|e| CliError::new(format!("设置 authorized_keys 权限失败: {}", e)))?;
    let status = SysCmd::new("chown")
        .args([
            "-R",
            &format!("{}:{}", username, username),
            ssh_dir.to_string_lossy().as_ref(),
        ])
        .status()
        .map_err(|e| CliError::new(format!("执行 chown 失败: {}", e)))?;
    if !status.success() {
        return Err(CliError::new("调整 .ssh 目录属主失败"));
    }
    Ok((auth_file, backup))
}

fn render_managed_config(plan: &SetupPlan) -> Result<String, CliError> {
    let mut lines = vec!["# vps-cli managed configuration".to_string()];
    if let Some(port) = plan.port {
        lines.push(format!("Port {}", port));
    }
    if plan.disable_root_login {
        lines.push("PermitRootLogin no".into());
    }
    if plan.disable_password_auth {
        lines.push("PasswordAuthentication no".into());
        if sshd_supports_keyword("KbdInteractiveAuthentication")? {
            lines.push("KbdInteractiveAuthentication no".into());
        }
        if sshd_supports_keyword("ChallengeResponseAuthentication")? {
            lines.push("ChallengeResponseAuthentication no".into());
        }
    }
    if let Some(ref allow_users) = plan.allow_users {
        lines.push(format!("AllowUsers {}", allow_users.trim()));
    }
    if plan.write_hardening {
        lines.push("PermitEmptyPasswords no".into());
        lines.push("X11Forwarding no".into());
        lines.push("MaxAuthTries 3".into());
        lines.push("LoginGraceTime 30".into());
        lines.push("UseDNS no".into());
    }
    Ok(lines.join("\n") + "\n")
}

fn validate_and_reload(plan: &SetupPlan) -> Result<(), CliError> {
    let output = SysCmd::new("sshd")
        .arg("-t")
        .output()
        .map_err(|e| CliError::new(format!("执行 sshd -t 失败: {}", e)))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "sshd -t 校验失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let effective = SysCmd::new("sshd")
        .arg("-T")
        .output()
        .map_err(|e| CliError::new(format!("执行 sshd -T 失败: {}", e)))?;
    if !effective.status.success() {
        return Err(CliError::new(format!(
            "sshd -T 校验失败: {}",
            String::from_utf8_lossy(&effective.stderr).trim()
        )));
    }
    let effective_text = String::from_utf8_lossy(&effective.stdout);
    verify_effective_config(plan, &effective_text)?;

    for service in ["ssh", "sshd"] {
        if let Ok(status) = SysCmd::new("systemctl").args(["reload", service]).status() {
            if status.success() {
                return Ok(());
            }
        }
    }
    Err(CliError::new("无法重载 SSH 服务"))
}

fn verify_effective_config(plan: &SetupPlan, effective: &str) -> Result<(), CliError> {
    if let Some(port) = plan.port {
        let ports = effective_values(effective, "port");
        if !ports.iter().any(|value| value == &port.to_string()) {
            return Err(CliError::new(format!("Port 未按预期生效，期望 {}", port)));
        }
    }
    if plan.disable_root_login {
        expect_effective(effective, "permitrootlogin", "no")?;
    }
    if plan.disable_password_auth {
        expect_effective(effective, "passwordauthentication", "no")?;
        if sshd_supports_keyword("KbdInteractiveAuthentication")? {
            expect_effective(effective, "kbdinteractiveauthentication", "no")?;
        }
        if sshd_supports_keyword("ChallengeResponseAuthentication")? {
            expect_effective(effective, "challengeresponseauthentication", "no")?;
        }
    }
    if let Some(ref allow_users) = plan.allow_users {
        let expected = allow_users.split_whitespace().collect::<Vec<_>>();
        let actual_line = effective_values(effective, "allowusers").join(" ");
        if actual_line.trim().is_empty() {
            return Err(CliError::new("AllowUsers 未按预期生效"));
        }
        for user in expected {
            if !actual_line.split_whitespace().any(|item| item == user) {
                return Err(CliError::new(format!("AllowUsers 未包含预期用户 {}", user)));
            }
        }
    }
    if plan.write_hardening {
        expect_effective(effective, "permitemptypasswords", "no")?;
        expect_effective(effective, "x11forwarding", "no")?;
        expect_effective(effective, "maxauthtries", "3")?;
        expect_effective(effective, "logingracetime", "30")?;
        expect_effective(effective, "usedns", "no")?;
    }
    Ok(())
}

fn restore_all(
    main_backup: &Option<PathBuf>,
    managed_backup: &Option<PathBuf>,
    auth_backup: &Option<PathBuf>,
    auth_target: Option<&Path>,
) -> Result<(), CliError> {
    if let Some(path) = main_backup {
        restore_backup(path, Path::new(SSHD_CONFIG))?;
    }
    if let Some(path) = managed_backup {
        restore_backup(path, Path::new(MANAGED_CONFIG))?;
    } else if Path::new(MANAGED_CONFIG).exists() {
        fs::remove_file(MANAGED_CONFIG)
            .map_err(|e| CliError::new(format!("删除托管配置失败: {}", e)))?;
    }
    if let (Some(path), Some(target)) = (auth_backup, auth_target) {
        restore_backup(path, target)?;
    }
    let _ = SysCmd::new("sshd").arg("-t").status();
    for service in ["ssh", "sshd"] {
        let _ = SysCmd::new("systemctl").args(["reload", service]).status();
    }
    Ok(())
}

fn configure_firewall(port: u16) -> Result<(), CliError> {
    let ufw_exists = SysCmd::new("sh")
        .arg("-c")
        .arg("command -v ufw >/dev/null 2>&1")
        .status()
        .map(|o| o.success())
        .unwrap_or(false);
    if ufw_exists {
        let status = SysCmd::new("ufw")
            .args(["allow", &format!("{}/tcp", port)])
            .status()
            .map_err(|e| CliError::new(format!("执行 ufw 失败: {}", e)))?;
        if status.success() {
            return Ok(());
        }
    }
    let firewalld_exists = SysCmd::new("sh")
        .arg("-c")
        .arg("command -v firewall-cmd >/dev/null 2>&1")
        .status()
        .map(|o| o.success())
        .unwrap_or(false);
    if firewalld_exists {
        let add = SysCmd::new("firewall-cmd")
            .args(["--permanent", &format!("--add-port={}/tcp", port)])
            .status()
            .map_err(|e| CliError::new(format!("执行 firewall-cmd 失败: {}", e)))?;
        let reload = SysCmd::new("firewall-cmd")
            .arg("--reload")
            .status()
            .map_err(|e| CliError::new(format!("执行 firewall-cmd --reload 失败: {}", e)))?;
        if add.success() && reload.success() {
            return Ok(());
        }
    }
    Ok(())
}

fn validate_public_key(path: &Path) -> Result<(), CliError> {
    let content =
        fs::read_to_string(path).map_err(|e| CliError::new(format!("读取公钥失败: {}", e)))?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(CliError::new("公钥文件为空"));
    }
    if !trimmed.starts_with("ssh-ed25519") && !trimmed.starts_with("ecdsa-") {
        return Err(CliError::new("仅允许 ssh-ed25519 或 ECDSA 公钥"));
    }
    let has_ssh_keygen = SysCmd::new("sh")
        .arg("-c")
        .arg("command -v ssh-keygen >/dev/null 2>&1")
        .status()
        .map(|o| o.success())
        .unwrap_or(false);
    if has_ssh_keygen {
        let output = SysCmd::new("ssh-keygen")
            .args(["-l", "-f", path.to_string_lossy().as_ref()])
            .output()
            .map_err(|e| CliError::new(format!("执行 ssh-keygen 失败: {}", e)))?;
        if !output.status.success() {
            return Err(CliError::new(format!(
                "ssh-keygen 校验公钥失败: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
    }
    Ok(())
}

fn user_home(username: &str) -> Result<PathBuf, CliError> {
    let output = SysCmd::new("bash")
        .arg("-c")
        .arg(format!("getent passwd {} | cut -d: -f6", username))
        .output()
        .map_err(|e| CliError::new(format!("获取用户主目录失败: {}", e)))?;
    if !output.status.success() {
        return Err(CliError::new(format!("无法获取用户 {} 的主目录", username)));
    }
    let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if home.is_empty() {
        Err(CliError::new(format!("用户 {} 的主目录为空", username)))
    } else {
        Ok(PathBuf::from(home))
    }
}

fn parse_port(value: String) -> Result<u16, CliError> {
    let port: u16 = value.parse().map_err(|_| CliError::new("端口必须为数字"))?;
    if !(1024..=65535).contains(&port) {
        return Err(CliError::new("端口必须在 1024-65535 之间"));
    }
    Ok(port)
}

fn is_port_in_use(port: u16) -> bool {
    let cmd = SysCmd::new("sh")
        .arg("-c")
        .arg(format!("ss -tuln | grep -w ':{}'", port))
        .output();
    if let Ok(output) = cmd {
        if !output.stdout.is_empty() {
            return true;
        }
    }
    let cmd2 = SysCmd::new("sh")
        .arg("-c")
        .arg(format!("netstat -tuln | grep -w ':{}'", port))
        .output();
    if let Ok(output) = cmd2 {
        if !output.stdout.is_empty() {
            return true;
        }
    }
    false
}

fn sshd_supports_keyword(keyword: &str) -> Result<bool, CliError> {
    let output = SysCmd::new("sshd")
        .arg("-T")
        .output()
        .map_err(|e| CliError::new(format!("执行 sshd -T 失败: {}", e)))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "无法检查 sshd 支持的配置项: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let lower = keyword.to_lowercase();
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.starts_with(&lower)))
}

fn effective_values(effective: &str, key: &str) -> Vec<String> {
    effective
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            if name == key {
                Some(parts.collect::<Vec<_>>().join(" "))
            } else {
                None
            }
        })
        .collect()
}

fn expect_effective(effective: &str, key: &str, expected: &str) -> Result<(), CliError> {
    let values = effective_values(effective, key);
    if values.iter().any(|value| value == expected) {
        Ok(())
    } else {
        Err(CliError::new(format!(
            "{} 未按预期生效，期望 {}，当前 {:?}",
            key, expected, values
        )))
    }
}

fn emit_success(
    format: OutputFormat,
    data: JsonValue,
    report: &OperationReport,
    breadcrumbs: Option<Vec<Breadcrumb>>,
) {
    match format {
        OutputFormat::Json => {
            let env = if let Some(crumbs) = breadcrumbs {
                OutputEnvelope::success(data)
                    .with_report(report)
                    .with_breadcrumbs(crumbs)
            } else {
                OutputEnvelope::success(data).with_report(report)
            };
            println!("{}", serde_json::to_string(&env).unwrap());
        }
        _ => {
            println!("{}", serde_json::to_string_pretty(&data).unwrap());
            if !report.warnings.is_empty() {
                println!("\n警告:");
                for item in &report.warnings {
                    println!("- {}", item);
                }
            }
            if !report.backups.is_empty() {
                println!("\n备份:");
                for item in &report.backups {
                    println!("- {}", item);
                }
            }
            if !report.changed_files.is_empty() {
                println!("\n变更文件:");
                for item in &report.changed_files {
                    println!("- {}", item);
                }
            }
            if let Some(rolled_back) = report.rolled_back {
                println!("\n已自动回滚: {}", if rolled_back { "是" } else { "否" });
            }
        }
    }
}

fn crumb(action: &str, cmd: &str) -> Breadcrumb {
    Breadcrumb {
        action: action.into(),
        cmd: cmd.into(),
    }
}
