use clap::{Arg, ArgAction, ArgMatches, Command};
use dialoguer::{Input, Select};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::Cursor;
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as SysCmd;
use tar::Archive;

use crate::safety::{
    append_backup, backup_path, current_timestamp, ensure_dir, is_interactive,
    require_confirmation, restore_backup,
};
use crate::utils::{Breadcrumb, CliError, OperationReport, OutputEnvelope, OutputFormat};

const MIERU_MANAGED_DIR: &str = "/etc/mieru-managed";
const MIERU_NODES_FILE: &str = "/etc/mieru-managed/nodes.json";
const MIERU_LINKS_FILE: &str = "/etc/mieru-managed/links.txt";
const MIERU_CONFIG_FILE: &str = "/etc/mieru-managed/mita_config.json";
const MIERU_BACKUP_DIR: &str = "/root/mieru-managed-backups";
const MIERU_CLIENT_HELPER_VERSION: &str = "3.32.0";
const MIERU_CLIENT_HELPER_DIR: &str = "/tmp/vps-cli-mieru-helper";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MieruNode {
    #[serde(rename = "type")]
    node_type: String,
    tag: String,
    host: String,
    port: u16,
    protocol: String,
    username: String,
    password: String,
    link: String,
    client: JsonValue,
    created_at: String,
}

pub fn cli() -> Command {
    Command::new("mieru")
        .about("管理 mita/mieru 服务端、节点与客户端参数")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("install")
                .about("安装或更新 mita 服务端")
                .arg(
                    Arg::new("version")
                        .long("version")
                        .value_name("VERSION")
                        .help("指定版本"),
                )
                .arg(
                    Arg::new("sha256")
                        .long("sha256")
                        .value_name("CHECKSUM")
                        .help("安装包 SHA256"),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .action(ArgAction::SetTrue)
                        .help("仅预览"),
                )
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认"),
                ),
        )
        .subcommand(
            Command::new("add-node")
                .about("添加 mieru/mita 节点")
                .arg(
                    Arg::new("host")
                        .long("host")
                        .value_name("HOST")
                        .help("客户端使用的公网域名或 IP"),
                )
                .arg(
                    Arg::new("port")
                        .long("port")
                        .value_name("PORT")
                        .help("监听端口"),
                )
                .arg(
                    Arg::new("protocol")
                        .long("protocol")
                        .value_name("PROTO")
                        .help("传输协议 tcp/udp"),
                )
                .arg(
                    Arg::new("username")
                        .long("username")
                        .value_name("USERNAME")
                        .help("用户名，默认自动生成"),
                )
                .arg(
                    Arg::new("password")
                        .long("password")
                        .value_name("PASSWORD")
                        .help("密码，默认自动生成"),
                )
                .arg(
                    Arg::new("tag")
                        .long("tag")
                        .value_name("TAG")
                        .help("节点标签"),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .action(ArgAction::SetTrue)
                        .help("仅预览"),
                )
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认"),
                )
                .arg(
                    Arg::new("show-secrets")
                        .long("show-secrets")
                        .action(ArgAction::SetTrue)
                        .help("显示敏感链接与客户端 JSON"),
                ),
        )
        .subcommand(Command::new("list-nodes").about("查看节点，安全视图"))
        .subcommand(
            Command::new("show-links")
                .about("兼容入口：查看 simple 链接和客户端 JSON")
                .arg(
                    Arg::new("id")
                        .long("id")
                        .value_name("TAG")
                        .help("仅查看指定标签"),
                )
                .arg(
                    Arg::new("show-secrets")
                        .long("show-secrets")
                        .action(ArgAction::SetTrue)
                        .help("确认展示敏感信息"),
                ),
        )
        .subcommand(
            Command::new("show-simple-links")
                .about("查看 simple 分享链接 mierus://")
                .arg(
                    Arg::new("id")
                        .long("id")
                        .value_name("TAG")
                        .help("仅查看指定标签"),
                )
                .arg(
                    Arg::new("show-secrets")
                        .long("show-secrets")
                        .action(ArgAction::SetTrue)
                        .help("确认展示敏感信息"),
                ),
        )
        .subcommand(
            Command::new("show-standard-links")
                .about("查看标准分享链接 mieru://")
                .arg(
                    Arg::new("id")
                        .long("id")
                        .value_name("TAG")
                        .help("仅查看指定标签"),
                )
                .arg(
                    Arg::new("show-secrets")
                        .long("show-secrets")
                        .action(ArgAction::SetTrue)
                        .help("确认展示敏感信息"),
                ),
        )
        .subcommand(Command::new("status").about("查看 mita 状态与配置摘要"))
        .subcommand(Command::new("start").about("启动 mita"))
        .subcommand(Command::new("stop").about("停止 mita"))
        .subcommand(
            Command::new("show-config").about("查看 mita 配置").arg(
                Arg::new("sensitive")
                    .long("sensitive")
                    .action(ArgAction::SetTrue)
                    .help("显示完整配置"),
            ),
        )
}

pub fn handle_mieru(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    match matches.subcommand() {
        Some(("install", sub)) => install_mita(sub, format, no_input),
        Some(("add-node", sub)) => add_node(sub, format, no_input),
        Some(("list-nodes", _)) => list_nodes(format),
        Some(("show-links", sub)) => show_links(sub, format, no_input),
        Some(("show-simple-links", sub)) => show_simple_links(sub, format),
        Some(("show-standard-links", sub)) => show_standard_links(sub, format),
        Some(("status", _)) => mita_status(format),
        Some(("start", _)) => mita_action("start", format),
        Some(("stop", _)) => mita_action("stop", format),
        Some(("show-config", sub)) => show_config(sub, format),
        _ => Err(CliError::new("未知子命令")),
    }
}

fn install_mita(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let version = matches
        .get_one::<String>("version")
        .cloned()
        .unwrap_or_else(|| "3.32.0".into());
    let sha256 = matches.get_one::<String>("sha256").cloned();
    let dry_run = matches.get_flag("dry-run");
    let confirm = matches.get_flag("confirm");
    let interactive = is_interactive(no_input);
    require_confirmation(
        "确认安装或更新 mita 服务端？",
        interactive,
        dry_run,
        confirm,
    )?;

    let (pkg, url, installer) = detect_package_info(&version)?;
    if dry_run {
        emit_success(
            format,
            json!({"dry_run": true, "version": version, "package": pkg, "url": url}),
            &OperationReport::default(),
            Some(vec![
                crumb("执行安装", "vps-cli mieru install --confirm"),
                crumb("添加节点", "vps-cli mieru add-node --confirm"),
            ]),
        );
        return Ok(());
    }

    let tmp_dir = PathBuf::from(format!("/tmp/mita-install-{}", version));
    let _ = fs::remove_dir_all(&tmp_dir);
    ensure_dir(&tmp_dir)?;
    let pkg_path = tmp_dir.join(&pkg);
    download_file(&url, &pkg_path)?;
    if let Some(checksum) = sha256 {
        let actual = compute_sha256(&pkg_path)?;
        if actual.to_lowercase() != checksum.to_lowercase() {
            return Err(CliError::new(format!(
                "mita 安装包 SHA256 校验失败：期望 {}，实际 {}",
                checksum, actual
            )));
        }
    }
    install_package(&pkg_path, installer)?;
    let _ = SysCmd::new("systemctl").args(["enable", "mita"]).status();
    init_state()?;
    emit_success(
        format,
        json!({"installed": true, "version": version, "package": pkg}),
        &OperationReport::default(),
        Some(vec![
            crumb("添加节点", "vps-cli mieru add-node --confirm"),
            crumb("查看状态", "vps-cli mieru status"),
        ]),
    );
    Ok(())
}

fn add_node(matches: &ArgMatches, format: OutputFormat, no_input: bool) -> Result<(), CliError> {
    let interactive = is_interactive(no_input);
    let dry_run = matches.get_flag("dry-run");
    let confirm = matches.get_flag("confirm");
    let show_secrets = matches.get_flag("show-secrets");
    let host = required_or_prompt(
        matches.get_one::<String>("host").cloned(),
        "请输入客户端使用的服务器公网域名/IP",
        interactive,
    )?;
    let port = parse_port(required_or_prompt(
        matches.get_one::<String>("port").cloned(),
        "请输入 mita 监听端口",
        interactive,
    )?)?;
    let protocol = resolve_protocol(matches.get_one::<String>("protocol").cloned(), interactive)?;
    if protocol != "TCP" && protocol != "UDP" {
        return Err(CliError::new("mieru 协议仅支持 TCP 或 UDP"));
    }
    let username = matches
        .get_one::<String>("username")
        .cloned()
        .unwrap_or_else(|| format!("u_{}", short_hex(4)));
    let password = matches
        .get_one::<String>("password")
        .cloned()
        .unwrap_or_else(|| random_b64url(24));
    let tag = matches
        .get_one::<String>("tag")
        .cloned()
        .unwrap_or_else(|| {
            format!(
                "mieru-{}-{}-{}",
                protocol.to_lowercase(),
                port,
                short_hex(3)
            )
        });
    let link = build_simple_link(&username, &password, &host, port, &protocol);
    let client_json = build_client_config(&host, port, &protocol, &username, &password);
    let node = MieruNode {
        node_type: "mieru".into(),
        tag: tag.clone(),
        host: host.clone(),
        port,
        protocol: protocol.clone(),
        username: username.clone(),
        password: password.clone(),
        link: link.clone(),
        client: client_json.clone(),
        created_at: iso_now(),
    };
    require_confirmation("确认添加 mieru/mita 节点？", interactive, dry_run, confirm)?;
    if dry_run {
        let mut report = OperationReport::default();
        if show_secrets {
            report.sensitive = Some(true);
        }
        emit_success(
            format,
            json!({
                "dry_run": true,
                "tag": tag,
                "protocol": protocol,
                "port": port,
                "host": host,
                "sensitive_output_split": true,
                "next_steps": {
                    "show_simple_links": "vps-cli mieru show-simple-links --show-secrets",
                    "show_standard_links": "vps-cli mieru show-standard-links --show-secrets"
                }
            }),
            &report,
            Some(vec![
                crumb("执行写入", "vps-cli mieru add-node --confirm"),
                crumb(
                    "查看 simple 链接",
                    "vps-cli mieru show-simple-links --show-secrets",
                ),
            ]),
        );
        return Ok(());
    }

    ensure_mita_exists()?;
    init_state()?;
    let mut nodes = read_nodes()?;
    if nodes
        .iter()
        .any(|n| n.tag == tag || (n.port == port && n.protocol == protocol))
    {
        return Err(CliError::new("该标签或端口/协议组合已存在"));
    }
    nodes.push(node.clone());
    let report = write_nodes_and_apply(&nodes)?;
    let mut report = report;
    if show_secrets {
        report.sensitive = Some(true);
        report
            .warnings
            .push("已按独立命令拆分敏感输出；请使用 `show-simple-links` 或 `show-standard-links` 查看链接。".into());
    }
    let mut data = json!({
        "added": tag,
        "protocol": protocol,
        "port": port,
        "host": host,
        "sensitive_output_split": true
    });
    if show_secrets {
        data["next_steps"] = json!({
            "show_simple_links": "vps-cli mieru show-simple-links --show-secrets",
            "show_standard_links": "vps-cli mieru show-standard-links --show-secrets"
        });
    }
    emit_success(
        format,
        data,
        &report,
        Some(vec![
            crumb("查看节点", "vps-cli mieru list-nodes"),
            crumb(
                "查看 simple 链接",
                "vps-cli mieru show-simple-links --show-secrets",
            ),
            crumb(
                "查看标准链接",
                "vps-cli mieru show-standard-links --show-secrets",
            ),
        ]),
    );
    Ok(())
}

fn list_nodes(format: OutputFormat) -> Result<(), CliError> {
    init_state()?;
    let data = read_nodes()?
        .into_iter()
        .map(|item| json!({"tag": item.tag, "protocol": item.protocol, "port": item.port, "host": item.host}))
        .collect::<Vec<_>>();
    emit_success(
        format,
        json!(data),
        &OperationReport::default(),
        Some(vec![crumb(
            "查看 simple 链接",
            "vps-cli mieru show-simple-links --show-secrets",
        )]),
    );
    Ok(())
}

fn show_links(matches: &ArgMatches, format: OutputFormat, no_input: bool) -> Result<(), CliError> {
    let show_secrets = matches.get_flag("show-secrets");
    let id = matches.get_one::<String>("id").cloned();
    let nodes = read_nodes()?;
    if !show_secrets {
        let data = nodes
            .iter()
            .filter(|item| id.as_ref().map(|wanted| wanted == &item.tag).unwrap_or(true))
            .map(|item| json!({"tag": item.tag, "host": item.host, "port": item.port, "protocol": item.protocol}))
            .collect::<Vec<_>>();
        emit_success(
            format,
            json!({"safe_view": true, "nodes": data, "tips": ["如需查看 simple 分享链接，请使用 `vps-cli mieru show-simple-links --show-secrets`", "如需查看标准分享链接，请使用 `vps-cli mieru show-standard-links --show-secrets`"]}),
            &OperationReport::default(),
            Some(vec![
                crumb(
                    "显示 simple 链接",
                    "vps-cli mieru show-simple-links --show-secrets",
                ),
                crumb(
                    "显示标准链接",
                    "vps-cli mieru show-standard-links --show-secrets",
                ),
            ]),
        );
        return Ok(());
    }
    require_confirmation(
        "当前输出将包含敏感凭据，确认继续？",
        is_interactive(no_input),
        false,
        true,
    )?;
    let mut report = OperationReport::default();
    report.sensitive = Some(true);
    report
        .warnings
        .push("当前输出包含敏感凭据，不应贴入公开日志。".into());
    report.warnings.push(
        "当前命令是兼容入口，会同时返回 simple 链接和客户端 JSON；如需标准分享链接，请使用 `vps-cli mieru show-standard-links --show-secrets`。"
            .into(),
    );
    let data = nodes
        .iter()
        .filter(|item| {
            id.as_ref()
                .map(|wanted| wanted == &item.tag)
                .unwrap_or(true)
        })
        .map(|item| json!({"tag": item.tag, "simple_link": item.link, "client_json": item.client}))
        .collect::<Vec<_>>();
    emit_success(format, json!(data), &report, None);
    Ok(())
}

fn show_simple_links(matches: &ArgMatches, format: OutputFormat) -> Result<(), CliError> {
    let show_secrets = matches.get_flag("show-secrets");
    let id = matches.get_one::<String>("id").cloned();
    let nodes = read_nodes()?;
    if !show_secrets {
        let data = nodes
            .iter()
            .filter(|item| id.as_ref().map(|wanted| wanted == &item.tag).unwrap_or(true))
            .map(|item| json!({"tag": item.tag, "host": item.host, "port": item.port, "protocol": item.protocol}))
            .collect::<Vec<_>>();
        emit_success(
            format,
            json!({"safe_view": true, "nodes": data, "tips": ["如需查看 simple 分享链接，请显式传入 --show-secrets"]}),
            &OperationReport::default(),
            Some(vec![crumb(
                "显示 simple 链接",
                "vps-cli mieru show-simple-links --show-secrets",
            )]),
        );
        return Ok(());
    }
    let mut report = OperationReport::default();
    report.sensitive = Some(true);
    report
        .warnings
        .push("当前输出包含敏感凭据，不应贴入公开日志。".into());
    let data = nodes
        .iter()
        .filter(|item| {
            id.as_ref()
                .map(|wanted| wanted == &item.tag)
                .unwrap_or(true)
        })
        .map(|item| {
            json!({
                "tag": item.tag,
                "simple_link": item.link
            })
        })
        .collect::<Vec<_>>();
    emit_success(format, json!(data), &report, None);
    Ok(())
}

fn show_standard_links(matches: &ArgMatches, format: OutputFormat) -> Result<(), CliError> {
    let show_secrets = matches.get_flag("show-secrets");
    let id = matches.get_one::<String>("id").cloned();
    let nodes = read_nodes()?;
    if !show_secrets {
        let data = nodes
            .iter()
            .filter(|item| id.as_ref().map(|wanted| wanted == &item.tag).unwrap_or(true))
            .map(|item| json!({"tag": item.tag, "host": item.host, "port": item.port, "protocol": item.protocol}))
            .collect::<Vec<_>>();
        emit_success(
            format,
            json!({"safe_view": true, "nodes": data, "tips": ["如需查看标准分享链接，请显式传入 --show-secrets"]}),
            &OperationReport::default(),
            Some(vec![crumb(
                "显示标准链接",
                "vps-cli mieru show-standard-links --show-secrets",
            )]),
        );
        return Ok(());
    }
    let mut report = OperationReport::default();
    report.sensitive = Some(true);
    report
        .warnings
        .push("当前输出包含完整标准分享链接，不应贴入公开日志。".into());
    let data = nodes
        .iter()
        .filter(|item| {
            id.as_ref()
                .map(|wanted| wanted == &item.tag)
                .unwrap_or(true)
        })
        .map(|item| {
            build_standard_link(&item.client).map(|link| {
                json!({
                    "tag": item.tag,
                    "standard_link": link
                })
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    emit_success(format, json!(data), &report, None);
    Ok(())
}

fn show_config(matches: &ArgMatches, format: OutputFormat) -> Result<(), CliError> {
    init_state()?;
    let data = fs::read_to_string(MIERU_CONFIG_FILE).unwrap_or_else(|_| "{}".into());
    let parsed: JsonValue = serde_json::from_str(&data).unwrap_or_else(|_| json!({}));
    if matches.get_flag("sensitive") {
        let mut report = OperationReport::default();
        report.sensitive = Some(true);
        report
            .warnings
            .push("完整配置包含凭据与端口绑定，不应贴入公开日志。".into());
        emit_success(format, parsed, &report, None);
    } else {
        emit_success(
            format,
            json!({
                "config": MIERU_CONFIG_FILE,
                "port_bindings": parsed.get("portBindings").cloned().unwrap_or_else(|| json!([])),
                "users_count": parsed.get("users").and_then(|v| v.as_array()).map(|v| v.len()).unwrap_or(0),
                "tips": ["如需查看完整配置，请显式传入 --sensitive"]
            }),
            &OperationReport::default(),
            Some(vec![crumb(
                "查看完整配置",
                "vps-cli mieru show-config --sensitive",
            )]),
        );
    }
    Ok(())
}

fn mita_status(format: OutputFormat) -> Result<(), CliError> {
    let systemd = SysCmd::new("systemctl")
        .args(["is-active", "mita"])
        .output();
    let mita_status = SysCmd::new("mita").arg("status").output();
    let mut warnings = vec![];
    let service = systemd
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into());
    let client_status = mita_status
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| {
            warnings.push("未检测到 mita 命令，或无法读取 mita status。".into());
            "".into()
        });
    let mut report = OperationReport::default();
    report.warnings = warnings;
    emit_success(
        format,
        json!({"systemd_status": service, "mita_status": client_status}),
        &report,
        None,
    );
    Ok(())
}

fn mita_action(action: &str, format: OutputFormat) -> Result<(), CliError> {
    ensure_mita_exists()?;
    let ok = match action {
        "start" => SysCmd::new("mita").arg("start").status(),
        "stop" => SysCmd::new("mita").arg("stop").status(),
        _ => return Err(CliError::new("未知操作")),
    }
    .map_err(|e| CliError::new(format!("执行 mita {} 失败: {}", action, e)))?;
    if !ok.success() {
        return Err(CliError::new(format!("mita {} 失败", action)));
    }
    emit_success(
        format,
        json!({action: true}),
        &OperationReport::default(),
        Some(vec![crumb("查看状态", "vps-cli mieru status")]),
    );
    Ok(())
}

fn detect_package_info(version: &str) -> Result<(String, String, &'static str), CliError> {
    let arch = match std::env::consts::ARCH {
        "x86_64" | "amd64" => "amd64",
        "aarch64" => "arm64",
        other => {
            return Err(CliError::new(format!(
                "当前架构暂不支持 mita 安装包：{}",
                other
            )))
        }
    };
    if Path::new("/usr/bin/dpkg").exists() {
        let pkg = format!("mita_{}_{}.deb", version, arch);
        let url = format!(
            "https://github.com/enfein/mieru/releases/download/v{}/{}",
            version, pkg
        );
        Ok((pkg, url, "dpkg"))
    } else if Path::new("/usr/bin/rpm").exists() {
        let pkg = format!("mita-{}-1.{}.rpm", version, arch);
        let url = format!(
            "https://github.com/enfein/mieru/releases/download/v{}/{}",
            version, pkg
        );
        Ok((pkg, url, "rpm"))
    } else {
        Err(CliError::new("未检测到 dpkg 或 rpm，无法安装 mita"))
    }
}

fn install_package(pkg_path: &Path, installer: &str) -> Result<(), CliError> {
    let status = match installer {
        "dpkg" => SysCmd::new("dpkg")
            .args(["-i", pkg_path.to_string_lossy().as_ref()])
            .status(),
        "rpm" => SysCmd::new("rpm")
            .args(["-Uvh", pkg_path.to_string_lossy().as_ref()])
            .status(),
        _ => return Err(CliError::new("未知安装器")),
    }
    .map_err(|e| CliError::new(format!("安装 mita 失败: {}", e)))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::new("安装 mita 失败"))
    }
}

fn init_state() -> Result<(), CliError> {
    ensure_dir(Path::new(MIERU_MANAGED_DIR))?;
    if !Path::new(MIERU_NODES_FILE).exists() {
        fs::write(MIERU_NODES_FILE, "[]")
            .map_err(|e| CliError::new(format!("初始化 nodes.json 失败: {}", e)))?;
    }
    if !Path::new(MIERU_LINKS_FILE).exists() {
        fs::write(MIERU_LINKS_FILE, "")
            .map_err(|e| CliError::new(format!("初始化 links.txt 失败: {}", e)))?;
    }
    Ok(())
}

fn read_nodes() -> Result<Vec<MieruNode>, CliError> {
    init_state()?;
    let file = fs::File::open(MIERU_NODES_FILE)
        .map_err(|e| CliError::new(format!("读取 mieru 节点失败: {}", e)))?;
    serde_json::from_reader(file).map_err(|e| CliError::new(format!("解析 mieru 节点失败: {}", e)))
}

fn write_nodes_and_apply(nodes: &[MieruNode]) -> Result<OperationReport, CliError> {
    ensure_dir(Path::new(MIERU_BACKUP_DIR))?;
    let nodes_backup = backup_path(
        Path::new(MIERU_NODES_FILE),
        Path::new(MIERU_BACKUP_DIR),
        "nodes",
    )?;
    let config_backup = backup_path(
        Path::new(MIERU_CONFIG_FILE),
        Path::new(MIERU_BACKUP_DIR),
        "config",
    )?;
    write_json(MIERU_NODES_FILE, nodes)?;
    rebuild_links_file(nodes)?;
    let mita_config = json!({
        "portBindings": nodes.iter().map(|n| json!({"port": n.port, "protocol": n.protocol})).collect::<Vec<_>>(),
        "users": nodes.iter().map(|n| json!({"name": n.username, "password": n.password})).collect::<Vec<_>>(),
        "loggingLevel": "INFO",
        "mtu": 1400
    });
    write_json(MIERU_CONFIG_FILE, &mita_config)?;
    let output = SysCmd::new("mita")
        .args(["apply", "config", MIERU_CONFIG_FILE])
        .output()
        .map_err(|e| CliError::new(format!("执行 mita apply config 失败: {}", e)))?;
    if !output.status.success() {
        if let Some(path) = &nodes_backup {
            let _ = restore_backup(path, Path::new(MIERU_NODES_FILE));
        }
        if let Some(path) = &config_backup {
            let _ = restore_backup(path, Path::new(MIERU_CONFIG_FILE));
        }
        return Err(CliError::new(format!(
            "mita apply config 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let _ = SysCmd::new("mita").arg("stop").status();
    let _ = SysCmd::new("mita").arg("start").status();
    let mut report = OperationReport::default();
    report.changed_files.push(MIERU_NODES_FILE.into());
    report.changed_files.push(MIERU_LINKS_FILE.into());
    report.changed_files.push(MIERU_CONFIG_FILE.into());
    append_backup(&mut report, &nodes_backup);
    append_backup(&mut report, &config_backup);
    Ok(report)
}

fn rebuild_links_file(nodes: &[MieruNode]) -> Result<(), CliError> {
    let mut buf = String::new();
    for (idx, node) in nodes.iter().enumerate() {
        buf.push_str(&format!(
            "{}. [{}] {}\n   {}\n",
            idx + 1,
            node.tag,
            node.protocol,
            node.link
        ));
    }
    fs::write(MIERU_LINKS_FILE, buf)
        .map_err(|e| CliError::new(format!("重建 links.txt 失败: {}", e)))
}

fn write_json<P: AsRef<Path>, T: Serialize + ?Sized>(path: P, value: &T) -> Result<(), CliError> {
    let data = serde_json::to_vec_pretty(value)
        .map_err(|e| CliError::new(format!("序列化 JSON 失败: {}", e)))?;
    fs::write(path.as_ref(), data)
        .map_err(|e| CliError::new(format!("写入文件失败 {}: {}", path.as_ref().display(), e)))
}

fn ensure_mita_exists() -> Result<(), CliError> {
    let status = SysCmd::new("sh")
        .arg("-c")
        .arg("command -v mita >/dev/null 2>&1")
        .status();
    match status {
        Ok(st) if st.success() => Ok(()),
        _ => Err(CliError::new(
            "未检测到 mita，请先执行 `vps-cli mieru install`",
        )),
    }
}

fn resolve_protocol(value: Option<String>, interactive: bool) -> Result<String, CliError> {
    match value {
        Some(v) => Ok(v.to_uppercase()),
        None if interactive => {
            let options = ["TCP", "UDP"];
            let selection = Select::new()
                .with_prompt("请选择 mita 传输协议")
                .items(&options)
                .default(0)
                .interact()
                .map_err(|e| CliError::new(format!("读取协议选择失败: {}", e)))?;
            Ok(options[selection].to_string())
        }
        None => Ok("TCP".into()),
    }
}

fn required_or_prompt(
    value: Option<String>,
    prompt: &str,
    interactive: bool,
) -> Result<String, CliError> {
    match value {
        Some(v) if !v.trim().is_empty() => Ok(v),
        _ if interactive => Input::<String>::new()
            .with_prompt(prompt)
            .interact_text()
            .map_err(|e| CliError::new(format!("读取输入失败: {}", e))),
        _ => Err(CliError::new(format!("缺少必需参数：{}", prompt))),
    }
}

fn parse_port(value: String) -> Result<u16, CliError> {
    let port: u16 = value.parse().map_err(|_| CliError::new("端口必须为数字"))?;
    if port < 1025 {
        return Err(CliError::new("mieru/mita 端口范围必须为 1025-65535"));
    }
    Ok(port)
}

fn short_hex(bytes: usize) -> String {
    let output = SysCmd::new("openssl")
        .args(["rand", "-hex", &bytes.to_string()])
        .output();
    output
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{:x}", current_timestamp()))
}

fn random_b64url(length: usize) -> String {
    let output = SysCmd::new("openssl")
        .args(["rand", "-base64", "48"])
        .output();
    let raw = output
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| format!("seed-{}", current_timestamp()));
    raw.replace('+', "-")
        .replace('/', "_")
        .replace('=', "")
        .replace('\n', "")
        .chars()
        .take(length)
        .collect()
}

fn format_uri_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{}]", host)
    } else {
        host.to_string()
    }
}

fn strip_ip_brackets(host: &str) -> String {
    host.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string()
}

fn is_ip_host(host: &str) -> bool {
    strip_ip_brackets(host).parse::<IpAddr>().is_ok()
}

fn build_simple_link(
    username: &str,
    password: &str,
    host: &str,
    port: u16,
    protocol: &str,
) -> String {
    format!(
        "mierus://{}:{}@{}?profile=default&mtu=1400&multiplexing=MULTIPLEXING_HIGH&handshake-mode=HANDSHAKE_STANDARD&port={}&protocol={}",
        username,
        password,
        format_uri_host(host),
        port,
        protocol
    )
}

fn build_client_config(
    host: &str,
    port: u16,
    protocol: &str,
    username: &str,
    password: &str,
) -> JsonValue {
    let server = if is_ip_host(host) {
        json!({
            "ipAddress": strip_ip_brackets(host),
            "portBindings": [{"port": port, "protocol": protocol}]
        })
    } else {
        json!({
            "domainName": host,
            "portBindings": [{"port": port, "protocol": protocol}]
        })
    };
    json!({
        "profiles": [{
            "profileName": "default",
            "user": {"name": username, "password": password},
            "servers": [server],
            "mtu": 1400,
            "multiplexing": {"level": "MULTIPLEXING_HIGH"},
            "handshakeMode": "HANDSHAKE_STANDARD"
        }],
        "activeProfile": "default",
        "rpcPort": 8964,
        "socks5Port": 1080,
        "loggingLevel": "INFO",
        "socks5ListenLAN": false
    })
}

fn build_standard_link(client_config: &JsonValue) -> Result<String, CliError> {
    let binary = ensure_mieru_client_binary()?;
    let work_dir = env::temp_dir().join(format!("vps-cli-mieru-export-{}", current_timestamp()));
    let _ = fs::remove_dir_all(&work_dir);
    ensure_dir(&work_dir)?;
    let config_path = work_dir.join("client.json");
    write_json(&config_path, client_config)?;
    let apply = SysCmd::new(&binary)
        .env("HOME", &work_dir)
        .args(["apply", "config"])
        .arg(&config_path)
        .output()
        .map_err(|e| CliError::new(format!("执行 mieru apply config 失败: {}", e)))?;
    if !apply.status.success() {
        return Err(CliError::new(format!(
            "生成标准分享链接失败：{}",
            String::from_utf8_lossy(&apply.stderr).trim()
        )));
    }
    let export = SysCmd::new(&binary)
        .env("HOME", &work_dir)
        .args(["export", "config"])
        .output()
        .map_err(|e| CliError::new(format!("执行 mieru export config 失败: {}", e)))?;
    if !export.status.success() {
        return Err(CliError::new(format!(
            "导出标准分享链接失败：{}",
            String::from_utf8_lossy(&export.stderr).trim()
        )));
    }
    let link = String::from_utf8_lossy(&export.stdout).trim().to_string();
    if !link.starts_with("mieru://") {
        return Err(CliError::new("导出的标准分享链接格式不正确"));
    }
    Ok(link)
}

fn ensure_mieru_client_binary() -> Result<PathBuf, CliError> {
    let system_binary = SysCmd::new("sh")
        .arg("-c")
        .arg("command -v mieru >/dev/null 2>&1")
        .status();
    if matches!(system_binary, Ok(status) if status.success()) {
        return Ok(PathBuf::from("mieru"));
    }
    let arch = match std::env::consts::ARCH {
        "x86_64" | "amd64" => "amd64",
        "aarch64" => "arm64",
        other => {
            return Err(CliError::new(format!(
                "当前架构暂不支持导出标准 mieru 分享链接：{}",
                other
            )))
        }
    };
    if std::env::consts::OS != "linux" {
        return Err(CliError::new(
            "当前仅支持在 Linux VPS 上导出标准 mieru 分享链接",
        ));
    }
    let cache_dir = Path::new(MIERU_CLIENT_HELPER_DIR)
        .join(MIERU_CLIENT_HELPER_VERSION)
        .join(arch);
    let binary = cache_dir.join("mieru");
    if binary.exists() {
        return Ok(binary);
    }
    ensure_dir(&cache_dir)?;
    let archive_path = cache_dir.join("mieru.tar.gz");
    let url = format!(
        "https://github.com/enfein/mieru/releases/download/v{}/mieru_{}_linux_{}.tar.gz",
        MIERU_CLIENT_HELPER_VERSION, MIERU_CLIENT_HELPER_VERSION, arch
    );
    download_file(&url, &archive_path)?;
    let archive_bytes = fs::read(&archive_path)
        .map_err(|e| CliError::new(format!("读取 mieru 客户端压缩包失败: {}", e)))?;
    let tar = GzDecoder::new(Cursor::new(archive_bytes));
    let mut archive = Archive::new(tar);
    archive
        .unpack(&cache_dir)
        .map_err(|e| CliError::new(format!("解压 mieru 客户端失败: {}", e)))?;
    if !binary.exists() {
        return Err(CliError::new("解压后未找到 mieru 可执行文件"));
    }
    let mut perms = fs::metadata(&binary)
        .map_err(|e| CliError::new(format!("读取 mieru 客户端权限失败: {}", e)))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&binary, perms)
        .map_err(|e| CliError::new(format!("设置 mieru 客户端权限失败: {}", e)))?;
    Ok(binary)
}

fn iso_now() -> String {
    current_timestamp().to_string()
}

fn compute_sha256(path: &Path) -> Result<String, CliError> {
    let data = fs::read(path).map_err(|e| CliError::new(format!("读取文件失败: {}", e)))?;
    let mut hasher = Sha256::new();
    hasher.update(data);
    Ok(hex::encode(hasher.finalize()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_config_prefers_ip_address_for_ip_hosts() {
        let config = build_client_config("1.2.3.4", 8443, "TCP", "user", "pass");
        assert_eq!(config["profiles"][0]["servers"][0]["ipAddress"], "1.2.3.4");
        assert!(config["profiles"][0]["servers"][0]["domainName"].is_null());
    }

    #[test]
    fn client_config_uses_domain_name_for_domain_hosts() {
        let config = build_client_config("example.com", 8443, "TCP", "user", "pass");
        assert_eq!(
            config["profiles"][0]["servers"][0]["domainName"],
            "example.com"
        );
        assert!(config["profiles"][0]["servers"][0]["ipAddress"].is_null());
    }

    #[test]
    fn simple_link_contains_expected_scheme() {
        let link = build_simple_link("user", "pass", "1.2.3.4", 8443, "TCP");
        assert!(link.starts_with("mierus://"));
        assert!(link.contains("port=8443"));
        assert!(link.contains("protocol=TCP"));
    }
}
