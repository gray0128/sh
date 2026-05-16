use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{json, Value as JsonValue};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as SysCmd;

use crate::safety::{
    append_backup, backup_path, ensure_dir, is_interactive, require_confirmation, restore_backup,
};
use crate::utils::{Breadcrumb, CliError, OperationReport, OutputEnvelope, OutputFormat};

const RECLAIM_BACKUP_DIR: &str = "/root/vps-cli-reclaim-backups";
const SINGBOX_CONFIG_DIR: &str = "/etc/sing-box";
const SINGBOX_SERVICE_FILE: &str = "/etc/systemd/system/sing-box.service";
const SINGBOX_BINARY_PATH: &str = "/usr/local/bin/sing-box";
const SINGBOX_META_FILE: &str = "/etc/sing-box/vps-cli-nodes.json";
const MIERU_MANAGED_DIR: &str = "/etc/mieru-managed";
const TRUSTTUNNEL_INSTALLER_URL: &str =
    "https://raw.githubusercontent.com/TrustTunnel/TrustTunnel/refs/heads/master/scripts/install.sh";
const TRUSTTUNNEL_DEFAULT_INSTALL_DIR: &str = "/opt/trusttunnel";
const TRUSTTUNNEL_SERVICE_FILE: &str = "/etc/systemd/system/trusttunnel.service";

#[derive(Debug, Clone)]
struct BackupEntry {
    original_path: PathBuf,
    backup_path: Option<PathBuf>,
}

pub fn cli() -> Command {
    Command::new("reclaim")
        .about("集中管理高风险的卸载、清理与审计操作")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("singbox-uninstall")
                .about("卸载 sing-box 二进制和服务，保留配置")
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认"),
                ),
        )
        .subcommand(
            Command::new("singbox-purge")
                .about("清理 vps-cli 托管的 sing-box 文件")
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认"),
                ),
        )
        .subcommand(
            Command::new("audit-proxies").about("审计 233boy/sing-box/xray/v2ray 清理候选项"),
        )
        .subcommand(
            Command::new("cleanup-proxies")
                .about("清理代理候选项，可选继续清理 nginx/caddy")
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认"),
                ),
        )
        .subcommand(Command::new("audit-nginx").about("审计 nginx 清理候选项"))
        .subcommand(
            Command::new("cleanup-nginx").about("清理 nginx").arg(
                Arg::new("confirm")
                    .long("confirm")
                    .action(ArgAction::SetTrue)
                    .help("跳过交互确认"),
            ),
        )
        .subcommand(Command::new("audit-caddy").about("审计 caddy 清理候选项"))
        .subcommand(
            Command::new("cleanup-caddy").about("清理 caddy").arg(
                Arg::new("confirm")
                    .long("confirm")
                    .action(ArgAction::SetTrue)
                    .help("跳过交互确认"),
            ),
        )
        .subcommand(
            Command::new("mieru-uninstall")
                .about("卸载 mita/mieru 服务端包、服务和托管状态")
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认"),
                ),
        )
        .subcommand(
            Command::new("trusttunnel-uninstall")
                .about("卸载 TrustTunnel 安装目录内容，尽量保留 systemd 文件")
                .arg(
                    Arg::new("output-dir")
                        .long("output-dir")
                        .value_name("DIR")
                        .help("TrustTunnel 安装目录，默认 /opt/trusttunnel"),
                )
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认"),
                ),
        )
        .subcommand(
            Command::new("trusttunnel-purge")
                .about("彻底清理 TrustTunnel 安装目录与 systemd 文件")
                .arg(
                    Arg::new("output-dir")
                        .long("output-dir")
                        .value_name("DIR")
                        .help("TrustTunnel 安装目录，默认 /opt/trusttunnel"),
                )
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认"),
                ),
        )
}

pub fn handle_reclaim(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    match matches.subcommand() {
        Some(("singbox-uninstall", sub)) => singbox_uninstall(sub, format, no_input),
        Some(("singbox-purge", sub)) => singbox_purge(sub, format, no_input),
        Some(("audit-proxies", _)) => audit_proxies(format),
        Some(("cleanup-proxies", sub)) => cleanup_proxies(sub, format, no_input),
        Some(("audit-nginx", _)) => audit_web_server("nginx", format),
        Some(("cleanup-nginx", sub)) => cleanup_web_server("nginx", sub, format, no_input),
        Some(("audit-caddy", _)) => audit_web_server("caddy", format),
        Some(("cleanup-caddy", sub)) => cleanup_web_server("caddy", sub, format, no_input),
        Some(("mieru-uninstall", sub)) => mieru_uninstall(sub, format, no_input),
        Some(("trusttunnel-uninstall", sub)) => trusttunnel_uninstall(sub, format, no_input),
        Some(("trusttunnel-purge", sub)) => trusttunnel_purge(sub, format, no_input),
        _ => Err(CliError::new("未知子命令")),
    }
}

fn singbox_uninstall(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let interactive = is_interactive(no_input);
    let confirm = matches.get_flag("confirm");
    require_confirmation(
        "这将停止 sing-box 服务并删除二进制与 systemd 服务，确认继续？",
        interactive,
        false,
        confirm,
    )?;

    let mut report = OperationReport::default();
    let backups = backup_paths(
        &[SINGBOX_BINARY_PATH, SINGBOX_SERVICE_FILE],
        "singbox-uninstall",
        &mut report,
    )?;
    stop_disable_service("sing-box");
    if let Err(err) = remove_if_exists(Path::new(SINGBOX_BINARY_PATH)) {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "卸载 sing-box 失败，已尝试恢复已删除文件。",
        ));
    }
    if let Err(err) = remove_if_exists(Path::new(SINGBOX_SERVICE_FILE)) {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "卸载 sing-box 失败，已尝试恢复已删除文件。",
        ));
    }
    if let Err(err) = daemon_reload() {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "重载 systemd 失败，已尝试恢复已删除文件。",
        ));
    }

    report.changed_files.push(SINGBOX_BINARY_PATH.into());
    report.changed_files.push(SINGBOX_SERVICE_FILE.into());
    report.rolled_back = Some(false);
    emit_success(
        format,
        json!({
            "uninstalled": true,
            "kept_paths": [SINGBOX_CONFIG_DIR, SINGBOX_META_FILE],
        }),
        &report,
        Some(vec![crumb(
            "彻底清理托管文件",
            "vps-cli reclaim singbox-purge --confirm",
        )]),
    );
    Ok(())
}

fn singbox_purge(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let interactive = is_interactive(no_input);
    let confirm = matches.get_flag("confirm");
    require_confirmation(
        "危险操作：这将删除 vps-cli 托管的 sing-box 配置、元数据与服务文件，确认继续？",
        interactive,
        false,
        confirm,
    )?;

    let mut report = OperationReport::default();
    let backups = backup_paths(
        &[
            SINGBOX_BINARY_PATH,
            SINGBOX_SERVICE_FILE,
            SINGBOX_CONFIG_DIR,
        ],
        "singbox-purge",
        &mut report,
    )?;
    stop_disable_service("sing-box");
    if let Err(err) = remove_if_exists(Path::new(SINGBOX_BINARY_PATH)) {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "清理 sing-box 托管文件失败，已尝试恢复已删除文件。",
        ));
    }
    if let Err(err) = remove_if_exists(Path::new(SINGBOX_SERVICE_FILE)) {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "清理 sing-box 托管文件失败，已尝试恢复已删除文件。",
        ));
    }
    if let Err(err) = remove_if_exists(Path::new(SINGBOX_CONFIG_DIR)) {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "清理 sing-box 托管文件失败，已尝试恢复已删除文件。",
        ));
    }
    if let Err(err) = daemon_reload() {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "重载 systemd 失败，已尝试恢复已删除文件。",
        ));
    }

    report.changed_files.push(SINGBOX_BINARY_PATH.into());
    report.changed_files.push(SINGBOX_SERVICE_FILE.into());
    report.changed_files.push(SINGBOX_CONFIG_DIR.into());
    report.rolled_back = Some(false);
    emit_success(
        format,
        json!({"purged": true, "removed_paths": [SINGBOX_BINARY_PATH, SINGBOX_SERVICE_FILE, SINGBOX_CONFIG_DIR]}),
        &report,
        None,
    );
    Ok(())
}

fn audit_proxies(format: OutputFormat) -> Result<(), CliError> {
    let candidates = collect_proxy_candidates();
    emit_success(
        format,
        json!({"candidates": candidates}),
        &OperationReport::default(),
        Some(vec![crumb(
            "执行清理",
            "vps-cli reclaim cleanup-proxies --confirm",
        )]),
    );
    Ok(())
}

fn cleanup_proxies(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let candidates = collect_proxy_candidates();
    if candidates.is_empty() {
        let mut report = OperationReport::default();
        report.rolled_back = Some(false);
        emit_success(
            format,
            json!({"cleaned": [], "candidates": []}),
            &report,
            Some(vec![
                crumb("审计 nginx", "vps-cli reclaim audit-nginx"),
                crumb("审计 caddy", "vps-cli reclaim audit-caddy"),
            ]),
        );
        return Ok(());
    }

    let interactive = is_interactive(no_input);
    let confirm = matches.get_flag("confirm");
    preview_candidates("即将清理的代理候选项", &candidates, format);
    require_confirmation(
        &format!(
            "将尝试删除 {} 个代理候选路径/服务，确认继续？",
            candidates.len()
        ),
        interactive,
        false,
        confirm,
    )?;

    let mut report = OperationReport::default();
    report
        .warnings
        .push("cleanup-proxies 已展示候选项，请确认删除范围无误。".into());
    let backups = backup_candidates(&candidates, "proxy-cleanup", &mut report)?;
    let mut removed = vec![];
    for candidate in &candidates {
        if candidate["kind"] == json!("service") {
            if let Some(name) = candidate["name"].as_str() {
                stop_disable_service(name);
            }
        }
        if let Some(path) = candidate["path"].as_str() {
            let target = Path::new(path);
            if target.exists() {
                if let Err(err) = remove_if_exists(target) {
                    return Err(rollback_error(
                        err,
                        &backups,
                        &mut report,
                        "清理代理候选项失败，已尝试恢复已删除文件。",
                    ));
                }
                removed.push(path.to_string());
            }
        }
    }
    if let Err(err) = daemon_reload() {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "重载 systemd 失败，已尝试恢复已删除文件。",
        ));
    }
    report.changed_files = removed.clone();
    report.rolled_back = Some(false);
    emit_success(
        format,
        json!({"candidates": candidates, "cleaned": removed}),
        &report,
        Some(vec![
            crumb("审计 nginx", "vps-cli reclaim audit-nginx"),
            crumb("审计 caddy", "vps-cli reclaim audit-caddy"),
        ]),
    );
    Ok(())
}

fn audit_web_server(name: &str, format: OutputFormat) -> Result<(), CliError> {
    let candidates = collect_web_candidates(name);
    emit_success(
        format,
        json!({"server": name, "candidates": candidates}),
        &OperationReport::default(),
        Some(vec![crumb(
            "执行清理",
            &format!("vps-cli reclaim cleanup-{} --confirm", name),
        )]),
    );
    Ok(())
}

fn cleanup_web_server(
    name: &str,
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let interactive = is_interactive(no_input);
    let confirm = matches.get_flag("confirm");
    let candidates = collect_web_candidates(name);
    if candidates.is_empty() {
        let mut report = OperationReport::default();
        report.rolled_back = Some(false);
        emit_success(
            format,
            json!({"server": name, "cleaned": [], "candidates": []}),
            &report,
            None,
        );
        return Ok(());
    }
    preview_candidates(&format!("即将清理的 {} 候选项", name), &candidates, format);
    require_confirmation(
        &format!(
            "将尝试清理 {} 的 {} 个候选路径/服务，确认继续？",
            name,
            candidates.len()
        ),
        interactive,
        false,
        confirm,
    )?;
    let mut report = OperationReport::default();
    report.warnings.push(format!(
        "cleanup-{} 已展示候选项，请确认删除范围无误。",
        name
    ));
    let backups = backup_candidates(&candidates, &format!("{}-cleanup", name), &mut report)?;
    let mut removed = vec![];
    for candidate in &candidates {
        if candidate["kind"] == json!("service") {
            if let Some(service) = candidate["name"].as_str() {
                stop_disable_service(service);
            }
        }
        if let Some(path) = candidate["path"].as_str() {
            let target = Path::new(path);
            if target.exists() {
                if let Err(err) = remove_if_exists(target) {
                    return Err(rollback_error(
                        err,
                        &backups,
                        &mut report,
                        "清理 Web 服务候选项失败，已尝试恢复已删除文件。",
                    ));
                }
                removed.push(path.to_string());
            }
        }
    }
    if let Err(err) = daemon_reload() {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "重载 systemd 失败，已尝试恢复已删除文件。",
        ));
    }
    report.changed_files = removed.clone();
    report.rolled_back = Some(false);
    emit_success(
        format,
        json!({"server": name, "candidates": candidates, "cleaned": removed}),
        &report,
        None,
    );
    Ok(())
}

fn mieru_uninstall(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let interactive = is_interactive(no_input);
    let confirm = matches.get_flag("confirm");
    require_confirmation(
        "这将删除 mita/mieru 服务端包、服务文件和托管状态，确认继续？",
        interactive,
        false,
        confirm,
    )?;

    let mut report = OperationReport::default();
    let backups = backup_paths(
        &[
            MIERU_MANAGED_DIR,
            "/etc/systemd/system/mita.service",
            "/lib/systemd/system/mita.service",
            "/usr/lib/systemd/system/mita.service",
        ],
        "mieru-uninstall",
        &mut report,
    )?;
    stop_disable_service("mita");
    for path in [
        "/etc/systemd/system/mita.service",
        "/lib/systemd/system/mita.service",
        "/usr/lib/systemd/system/mita.service",
    ] {
        if let Err(err) = remove_if_exists(Path::new(path)) {
            return Err(rollback_error(
                err,
                &backups,
                &mut report,
                "卸载 mieru 托管文件失败，已尝试恢复已删除文件。",
            ));
        }
    }
    if let Err(err) = remove_if_exists(Path::new(MIERU_MANAGED_DIR)) {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "卸载 mieru 托管文件失败，已尝试恢复已删除文件。",
        ));
    }
    if let Err(err) = daemon_reload() {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "重载 systemd 失败，已尝试恢复已删除文件。",
        ));
    }
    uninstall_mita_package();
    run_systemctl_best_effort(&["reset-failed", "mita.service"]);

    report.changed_files.push(MIERU_MANAGED_DIR.into());
    report
        .changed_files
        .push("/etc/systemd/system/mita.service".into());
    report
        .changed_files
        .push("/lib/systemd/system/mita.service".into());
    report
        .changed_files
        .push("/usr/lib/systemd/system/mita.service".into());
    report.rolled_back = Some(false);
    emit_success(
        format,
        json!({"uninstalled": true, "target": "mita/mieru"}),
        &report,
        None,
    );
    Ok(())
}

fn trusttunnel_uninstall(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let output_dir = normalized_trusttunnel_dir(matches.get_one::<String>("output-dir"));
    let confirm = matches.get_flag("confirm");
    let interactive = is_interactive(no_input);
    trusttunnel_uninstall_shared(&output_dir, format, interactive, confirm)
}

fn trusttunnel_purge(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let output_dir = normalized_trusttunnel_dir(matches.get_one::<String>("output-dir"));
    let confirm = matches.get_flag("confirm");
    let interactive = is_interactive(no_input);
    trusttunnel_purge_shared(&output_dir, format, interactive, confirm)
}

pub fn trusttunnel_uninstall_shared(
    output_dir: &str,
    format: OutputFormat,
    interactive: bool,
    confirm: bool,
) -> Result<(), CliError> {
    require_confirmation(
        "这将停止 TrustTunnel 服务并卸载安装目录内容，确认继续？",
        interactive,
        false,
        confirm,
    )?;
    stop_disable_service("trusttunnel");
    run_trusttunnel_installer_uninstall(output_dir)?;
    let mut report = OperationReport::default();
    report.changed_files.push(output_dir.into());
    if Path::new(TRUSTTUNNEL_SERVICE_FILE).exists() {
        report.warnings.push(format!(
            "卸载后仍检测到 systemd 文件 {}，如需彻底删除请执行 `vps-cli trusttunnel purge --confirm` 或 `vps-cli reclaim trusttunnel-purge --confirm`。",
            TRUSTTUNNEL_SERVICE_FILE
        ));
    }
    emit_success(
        format,
        json!({
            "uninstalled": true,
            "target": "trusttunnel",
            "output_dir": output_dir,
            "service_file_exists": Path::new(TRUSTTUNNEL_SERVICE_FILE).exists(),
        }),
        &report,
        Some(vec![crumb(
            "彻底清理 systemd 文件",
            "vps-cli reclaim trusttunnel-purge --confirm",
        )]),
    );
    Ok(())
}

pub fn trusttunnel_purge_shared(
    output_dir: &str,
    format: OutputFormat,
    interactive: bool,
    confirm: bool,
) -> Result<(), CliError> {
    require_confirmation(
        "危险操作：这将删除 TrustTunnel 安装目录与 systemd 文件，确认继续？",
        interactive,
        false,
        confirm,
    )?;
    let mut report = OperationReport::default();
    let backups = backup_paths(
        &[output_dir, TRUSTTUNNEL_SERVICE_FILE],
        "trusttunnel-purge",
        &mut report,
    )?;
    stop_disable_service("trusttunnel");
    if let Err(err) = remove_if_exists(Path::new(output_dir)) {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "清理 TrustTunnel 安装目录失败，已尝试恢复已删除文件。",
        ));
    }
    if let Err(err) = remove_if_exists(Path::new(TRUSTTUNNEL_SERVICE_FILE)) {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "清理 TrustTunnel systemd 文件失败，已尝试恢复已删除文件。",
        ));
    }
    if let Err(err) = daemon_reload() {
        return Err(rollback_error(
            err,
            &backups,
            &mut report,
            "重载 systemd 失败，已尝试恢复已删除文件。",
        ));
    }
    report.changed_files.push(output_dir.into());
    report.changed_files.push(TRUSTTUNNEL_SERVICE_FILE.into());
    report.rolled_back = Some(false);
    emit_success(
        format,
        json!({
            "purged": true,
            "target": "trusttunnel",
            "removed_paths": [output_dir, TRUSTTUNNEL_SERVICE_FILE],
        }),
        &report,
        None,
    );
    Ok(())
}

fn collect_proxy_candidates() -> Vec<JsonValue> {
    let service_names = [
        "sing-box.service",
        "xray.service",
        "v2ray.service",
        "v2ray@server.service",
    ];
    let mut candidates = vec![];
    for service in service_names {
        if systemd_exists(service) {
            candidates.push(json!({"kind": "service", "name": service, "path": format!("/etc/systemd/system/{}", service)}));
        }
    }
    for path in [
        "/etc/sing-box",
        "/usr/local/bin/sing-box",
        "/usr/local/bin/xray",
        "/usr/local/bin/v2ray",
        "/etc/xray",
        "/etc/v2ray",
        "/var/log/sing-box",
        "/var/log/xray",
        "/var/log/v2ray",
        "/usr/local/etc/xray",
        "/usr/local/etc/v2ray",
    ] {
        if Path::new(path).exists() {
            candidates.push(json!({"kind": "path", "path": path}));
        }
    }
    dedup_candidates(candidates)
}

fn normalized_trusttunnel_dir(value: Option<&String>) -> String {
    let raw = value
        .map(|item| item.trim())
        .unwrap_or(TRUSTTUNNEL_DEFAULT_INSTALL_DIR);
    match raw {
        "" | "." | "/opt" => TRUSTTUNNEL_DEFAULT_INSTALL_DIR.to_string(),
        _ => raw.to_string(),
    }
}

fn run_trusttunnel_installer_uninstall(output_dir: &str) -> Result<(), CliError> {
    let tmp_dir = std::env::temp_dir().join(format!(
        "vps-cli-trusttunnel-uninstall-{}",
        crate::safety::current_timestamp()
    ));
    ensure_dir(&tmp_dir)?;
    let script_path = tmp_dir.join("install.sh");
    download_file(TRUSTTUNNEL_INSTALLER_URL, &script_path)?;
    let output = SysCmd::new("sh")
        .arg(&script_path)
        .args(["-u", "-o", output_dir, "-a", "y"])
        .output()
        .map_err(|e| CliError::new(format!("执行 TrustTunnel 官方卸载脚本失败: {}", e)))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CliError::new(format!(
            "TrustTunnel 官方卸载脚本失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn download_file(url: &str, dest: &Path) -> Result<(), CliError> {
    let response =
        reqwest::blocking::get(url).map_err(|e| CliError::new(format!("下载失败: {}", e)))?;
    if !response.status().is_success() {
        return Err(CliError::new(format!(
            "下载失败：HTTP {}",
            response.status()
        )));
    }
    let bytes = response
        .bytes()
        .map_err(|e| CliError::new(format!("读取下载内容失败: {}", e)))?;
    fs::write(dest, &bytes).map_err(|e| CliError::new(format!("写入下载文件失败: {}", e)))
}

fn collect_web_candidates(name: &str) -> Vec<JsonValue> {
    let mut candidates = vec![];
    let service = format!("{}.service", name);
    if systemd_exists(&service) {
        candidates.push(json!({"kind": "service", "name": service, "path": format!("/etc/systemd/system/{}", service)}));
    }
    let paths = match name {
        "nginx" => vec![
            "/etc/nginx",
            "/var/log/nginx",
            "/usr/sbin/nginx",
            "/usr/local/nginx",
        ],
        "caddy" => vec![
            "/etc/caddy",
            "/var/log/caddy",
            "/usr/bin/caddy",
            "/usr/local/bin/caddy",
        ],
        _ => vec![],
    };
    for path in paths {
        if Path::new(path).exists() {
            candidates.push(json!({"kind": "path", "path": path}));
        }
    }
    dedup_candidates(candidates)
}

fn dedup_candidates(mut items: Vec<JsonValue>) -> Vec<JsonValue> {
    items.sort_by(|a, b| {
        let a_key = a["path"]
            .as_str()
            .or_else(|| a["name"].as_str())
            .unwrap_or_default();
        let b_key = b["path"]
            .as_str()
            .or_else(|| b["name"].as_str())
            .unwrap_or_default();
        a_key.cmp(b_key)
    });
    items.dedup_by(|a, b| {
        let a_key = a["path"]
            .as_str()
            .or_else(|| a["name"].as_str())
            .unwrap_or_default();
        let b_key = b["path"]
            .as_str()
            .or_else(|| b["name"].as_str())
            .unwrap_or_default();
        a_key == b_key
    });
    items
}

fn stop_disable_service(name: &str) {
    run_systemctl_best_effort(&["stop", name]);
    run_systemctl_best_effort(&["disable", name]);
    run_systemctl_best_effort(&["reset-failed", name]);
}

fn uninstall_mita_package() {
    if Path::new("/usr/bin/dpkg").exists() {
        let _ = SysCmd::new("sh")
            .arg("-c")
            .arg("dpkg -s mita >/dev/null 2>&1 && DEBIAN_FRONTEND=noninteractive apt-get purge -y mita && DEBIAN_FRONTEND=noninteractive apt-get autoremove -y || true")
            .status();
    } else if Path::new("/usr/bin/rpm").exists() {
        let _ = SysCmd::new("sh")
            .arg("-c")
            .arg("rpm -q mita >/dev/null 2>&1 && (command -v dnf >/dev/null 2>&1 && dnf remove -y mita || command -v yum >/dev/null 2>&1 && yum remove -y mita || command -v zypper >/dev/null 2>&1 && zypper --non-interactive remove mita || rpm -e mita) || true")
            .status();
    }
}

fn is_benign_systemctl_stderr(stderr: &str) -> bool {
    let stderr = stderr.trim().to_ascii_lowercase();
    stderr.contains("not loaded")
        || stderr.contains("could not be found")
        || stderr.contains("does not exist")
        || stderr.contains("no files found for")
}

fn run_systemctl_best_effort(args: &[&str]) {
    let rendered = args.join(" ");
    match SysCmd::new("systemctl").args(args).output() {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() && !is_benign_systemctl_stderr(&stderr) {
                eprintln!("警告: `systemctl {}` 失败: {}", rendered, stderr.trim());
            }
        }
        Err(err) => {
            eprintln!("警告: 无法执行 `systemctl {}`: {}", rendered, err);
        }
    }
}

fn remove_if_exists(path: &Path) -> Result<(), CliError> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|e| CliError::new(format!("删除目录失败 {}: {}", path.display(), e)))
    } else {
        fs::remove_file(path)
            .map_err(|e| CliError::new(format!("删除文件失败 {}: {}", path.display(), e)))
    }
}

fn systemd_exists(unit: &str) -> bool {
    let output = SysCmd::new("systemctl").args(["status", unit]).output();
    output
        .map(|o| o.status.success() || !o.stderr.is_empty())
        .unwrap_or(false)
}

fn daemon_reload() -> Result<(), CliError> {
    let status = SysCmd::new("systemctl")
        .arg("daemon-reload")
        .status()
        .map_err(|e| CliError::new(format!("执行 systemctl daemon-reload 失败: {}", e)))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::new("systemctl daemon-reload 执行失败"))
    }
}

fn preview_candidates(title: &str, candidates: &[JsonValue], format: OutputFormat) {
    if matches!(format, OutputFormat::Json) {
        return;
    }
    println!("{}:", title);
    for item in candidate_descriptions(candidates) {
        println!("- {}", item);
    }
}

#[cfg(test)]
mod tests {
    use super::is_benign_systemctl_stderr;

    #[test]
    fn benign_systemctl_missing_unit_messages_are_ignored() {
        assert!(is_benign_systemctl_stderr(
            "Failed to reset failed state of unit sing-box.service: Unit sing-box.service not loaded."
        ));
        assert!(is_benign_systemctl_stderr(
            "Failed to disable unit: Unit file sing-box.service does not exist."
        ));
        assert!(is_benign_systemctl_stderr(
            "Unit sing-box.service could not be found."
        ));
    }

    #[test]
    fn unexpected_systemctl_errors_are_not_ignored() {
        assert!(!is_benign_systemctl_stderr(
            "Failed to connect to bus: No such file or directory"
        ));
    }
}

fn candidate_descriptions(candidates: &[JsonValue]) -> Vec<String> {
    candidates
        .iter()
        .map(|candidate| {
            if candidate["kind"] == json!("service") {
                let name = candidate["name"].as_str().unwrap_or("<unknown>");
                let path = candidate["path"].as_str().unwrap_or("<unknown>");
                format!("service {} ({})", name, path)
            } else {
                candidate["path"]
                    .as_str()
                    .map(|path| format!("path {}", path))
                    .unwrap_or_else(|| "path <unknown>".into())
            }
        })
        .collect()
}

fn backup_candidates(
    candidates: &[JsonValue],
    prefix: &str,
    report: &mut OperationReport,
) -> Result<Vec<BackupEntry>, CliError> {
    let mut paths = candidates
        .iter()
        .filter_map(|candidate| candidate["path"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let refs = paths.iter().map(|item| item.as_str()).collect::<Vec<_>>();
    backup_paths(&refs, prefix, report)
}

fn backup_paths(
    paths: &[&str],
    prefix: &str,
    report: &mut OperationReport,
) -> Result<Vec<BackupEntry>, CliError> {
    ensure_dir(Path::new(RECLAIM_BACKUP_DIR))?;
    let mut backups = Vec::with_capacity(paths.len());
    for path in paths {
        let path_ref = Path::new(path);
        let backup = backup_path(
            path_ref,
            Path::new(RECLAIM_BACKUP_DIR),
            &format!("{}-{}", prefix, sanitize_path(path)),
        )?;
        append_backup(report, &backup);
        backups.push(BackupEntry {
            original_path: path_ref.to_path_buf(),
            backup_path: backup,
        });
    }
    Ok(backups)
}

fn sanitize_path(path: &str) -> String {
    let sanitized = path.trim_start_matches('/').replace('/', "_");
    if sanitized.is_empty() {
        "root".into()
    } else {
        sanitized
    }
}

fn rollback_backups(entries: &[BackupEntry], report: &mut OperationReport) -> Result<(), CliError> {
    for entry in entries.iter().rev() {
        if let Some(ref backup) = entry.backup_path {
            restore_backup(backup, &entry.original_path)?;
        }
    }
    report.rolled_back = Some(true);
    Ok(())
}

fn rollback_error(
    err: CliError,
    backups: &[BackupEntry],
    report: &mut OperationReport,
    warning: &str,
) -> CliError {
    match rollback_backups(backups, report) {
        Ok(()) => {
            report.warnings.push(warning.into());
            err.with_warnings(vec![warning.into()]).with_report(report)
        }
        Err(rollback_err) => {
            report.rolled_back = Some(true);
            report
                .warnings
                .push(format!("{} 回滚阶段再次失败: {}", warning, rollback_err));
            CliError::new(format!("{}；回滚失败: {}", err.message, rollback_err))
                .with_warnings(report.warnings.clone())
                .with_report(report)
        }
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
