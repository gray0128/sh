use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{json, Value as JsonValue};
use std::fs;
use std::path::Path;
use std::process::Command as SysCmd;

use crate::safety::{append_backup, backup_path, ensure_dir, is_interactive, require_confirmation};
use crate::utils::{Breadcrumb, CliError, OperationReport, OutputEnvelope, OutputFormat};

const RECLAIM_BACKUP_DIR: &str = "/root/vps-cli-reclaim-backups";
const SINGBOX_CONFIG_DIR: &str = "/etc/sing-box";
const SINGBOX_SERVICE_FILE: &str = "/etc/systemd/system/sing-box.service";
const SINGBOX_BINARY_PATH: &str = "/usr/local/bin/sing-box";
const SINGBOX_META_FILE: &str = "/etc/sing-box/vps-cli-nodes.json";
const MIERU_MANAGED_DIR: &str = "/etc/mieru-managed";

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

    ensure_dir(Path::new(RECLAIM_BACKUP_DIR))?;
    let service_backup = backup_path(
        Path::new(SINGBOX_SERVICE_FILE),
        Path::new(RECLAIM_BACKUP_DIR),
        "singbox-service",
    )?;
    let meta_backup = backup_path(
        Path::new(SINGBOX_META_FILE),
        Path::new(RECLAIM_BACKUP_DIR),
        "singbox-meta",
    )?;
    stop_disable_service("sing-box");
    remove_if_exists(Path::new(SINGBOX_BINARY_PATH))?;
    remove_if_exists(Path::new(SINGBOX_SERVICE_FILE))?;
    let _ = SysCmd::new("systemctl").arg("daemon-reload").status();

    let mut report = OperationReport::default();
    report.changed_files.push(SINGBOX_BINARY_PATH.into());
    report.changed_files.push(SINGBOX_SERVICE_FILE.into());
    append_backup(&mut report, &service_backup);
    append_backup(&mut report, &meta_backup);
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

    ensure_dir(Path::new(RECLAIM_BACKUP_DIR))?;
    let cfg_backup = backup_path(
        Path::new(SINGBOX_CONFIG_DIR),
        Path::new(RECLAIM_BACKUP_DIR),
        "singbox-dir",
    )?;
    let service_backup = backup_path(
        Path::new(SINGBOX_SERVICE_FILE),
        Path::new(RECLAIM_BACKUP_DIR),
        "singbox-service",
    )?;
    stop_disable_service("sing-box");
    remove_if_exists(Path::new(SINGBOX_BINARY_PATH))?;
    remove_if_exists(Path::new(SINGBOX_SERVICE_FILE))?;
    remove_if_exists(Path::new(SINGBOX_CONFIG_DIR))?;
    let _ = SysCmd::new("systemctl").arg("daemon-reload").status();

    let mut report = OperationReport::default();
    report.changed_files.push(SINGBOX_BINARY_PATH.into());
    report.changed_files.push(SINGBOX_SERVICE_FILE.into());
    report.changed_files.push(SINGBOX_CONFIG_DIR.into());
    append_backup(&mut report, &cfg_backup);
    append_backup(&mut report, &service_backup);
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
    let interactive = is_interactive(no_input);
    let confirm = matches.get_flag("confirm");
    require_confirmation(
        &format!(
            "将尝试删除 {} 个代理候选路径/服务，确认继续？",
            candidates.len()
        ),
        interactive,
        false,
        confirm,
    )?;
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
                let _ = remove_if_exists(target);
                removed.push(path.to_string());
            }
        }
    }
    let mut report = OperationReport::default();
    report.changed_files = removed.clone();
    emit_success(
        format,
        json!({"cleaned": removed}),
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
                let _ = remove_if_exists(target);
                removed.push(path.to_string());
            }
        }
    }
    let mut report = OperationReport::default();
    report.changed_files = removed.clone();
    emit_success(
        format,
        json!({"server": name, "cleaned": removed}),
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

    ensure_dir(Path::new(RECLAIM_BACKUP_DIR))?;
    let managed_backup = backup_path(
        Path::new(MIERU_MANAGED_DIR),
        Path::new(RECLAIM_BACKUP_DIR),
        "mieru-dir",
    )?;
    stop_disable_service("mita");
    uninstall_mita_package();
    for path in [
        "/etc/systemd/system/mita.service",
        "/lib/systemd/system/mita.service",
        "/usr/lib/systemd/system/mita.service",
    ] {
        let _ = remove_if_exists(Path::new(path));
    }
    remove_if_exists(Path::new(MIERU_MANAGED_DIR))?;
    let _ = SysCmd::new("systemctl").arg("daemon-reload").status();
    let _ = SysCmd::new("systemctl")
        .args(["reset-failed", "mita.service"])
        .status();

    let mut report = OperationReport::default();
    report.changed_files.push(MIERU_MANAGED_DIR.into());
    append_backup(&mut report, &managed_backup);
    emit_success(
        format,
        json!({"uninstalled": true, "target": "mita/mieru"}),
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
    let _ = SysCmd::new("systemctl").args(["stop", name]).status();
    let _ = SysCmd::new("systemctl").args(["disable", name]).status();
    let _ = SysCmd::new("systemctl")
        .args(["reset-failed", name])
        .status();
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
        }
    }
}

fn crumb(action: &str, cmd: &str) -> Breadcrumb {
    Breadcrumb {
        action: action.into(),
        cmd: cmd.into(),
    }
}
