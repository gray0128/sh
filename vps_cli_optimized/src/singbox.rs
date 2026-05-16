use clap::{Arg, ArgAction, ArgMatches, Command};
use dialoguer::{Confirm, Input, Select};
use flate2::read::GzDecoder;
use hex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::fs;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Command as SysCmd;
use tar::Archive;

use crate::safety::{
    append_backup, backup_path, current_timestamp, ensure_dir, is_interactive,
    require_confirmation, restore_backup,
};
use crate::utils::{Breadcrumb, CliError, OperationReport, OutputEnvelope, OutputFormat};

const SINGBOX_CONFIG_PATH: &str = "/etc/sing-box/config.json";
const SINGBOX_META_PATH: &str = "/etc/sing-box/vps-cli-nodes.json";
const SINGBOX_BACKUP_DIR: &str = "/root/sing-box-backups";
const SINGBOX_SERVICE_FILE: &str = "/etc/systemd/system/sing-box.service";
const SINGBOX_BINARY_PATH: &str = "/usr/local/bin/sing-box";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeMeta {
    tag: String,
    #[serde(rename = "type")]
    node_type: String,
    server: String,
    port: u16,
    network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_json: Option<JsonValue>,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeSummary {
    id: String,
    #[serde(rename = "type")]
    node_type: String,
    port: u16,
    network: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

pub fn cli() -> Command {
    Command::new("singbox")
        .about("管理 sing-box 服务端安装、协议节点与运行状态")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("install")
                .about("安装或更新 sing-box")
                .arg(
                    Arg::new("version")
                        .long("version")
                        .value_name("VERSION")
                        .help("指定安装的 sing-box 版本，例如 1.8.3")
                        .required(false),
                )
                .arg(
                    Arg::new("sha256")
                        .long("sha256")
                        .value_name("CHECKSUM")
                        .help("sing-box 二进制的 SHA256 校验值，可选")
                        .required(false),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .action(ArgAction::SetTrue)
                        .help("预览安装步骤，但不实际执行"),
                )
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认，直接安装"),
                ),
        )
        .subcommand(
            Command::new("add-node")
                .about("从 JSON 文件导入一个或多个入站节点")
                .arg(
                    Arg::new("type")
                        .long("type")
                        .value_name("TYPE")
                        .help("节点类型标识，仅用于输出说明")
                        .required(true),
                )
                .arg(
                    Arg::new("config-file")
                        .long("config-file")
                        .value_name("FILE")
                        .help("包含节点配置的 JSON 文件")
                        .required(true),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .action(ArgAction::SetTrue)
                        .help("仅预览配置变更，不实际写入"),
                )
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认，直接应用配置"),
                ),
        )
        .subcommand(
            protocol_command("add-vless-reality", "添加 VLESS + Reality 节点", "tcp")
                .arg(
                    Arg::new("uuid")
                        .long("uuid")
                        .value_name("UUID")
                        .help("指定 UUID，默认自动生成"),
                )
                .arg(
                    Arg::new("server-name")
                        .long("server-name")
                        .value_name("SNI")
                        .help("Reality 握手域名"),
                )
                .arg(
                    Arg::new("short-id")
                        .long("short-id")
                        .value_name("HEX")
                        .help("Reality short id，默认自动生成"),
                )
                .arg(
                    Arg::new("private-key")
                        .long("private-key")
                        .value_name("KEY")
                        .help("Reality 私钥；不提供时自动生成"),
                )
                .arg(
                    Arg::new("public-key")
                        .long("public-key")
                        .value_name("KEY")
                        .help("Reality 公钥；自动生成私钥时会同时生成"),
                )
                .arg(
                    Arg::new("show-secrets")
                        .long("show-secrets")
                        .action(ArgAction::SetTrue)
                        .help("输出敏感链接与客户端 JSON"),
                ),
        )
        .subcommand(
            tls_protocol_command("add-trojan-tls", "添加 Trojan + TLS 节点", "tcp").arg(
                Arg::new("password")
                    .long("password")
                    .value_name("PASSWORD")
                    .help("指定 Trojan 密码，默认自动生成"),
            ),
        )
        .subcommand(
            tls_protocol_command("add-hysteria2-tls", "添加 Hysteria2 + TLS 节点", "udp")
                .arg(
                    Arg::new("password")
                        .long("password")
                        .value_name("PASSWORD")
                        .help("指定密码，默认自动生成"),
                )
                .arg(
                    Arg::new("obfs-password")
                        .long("obfs-password")
                        .value_name("PASSWORD")
                        .help("指定 obfs 密码，默认自动生成"),
                ),
        )
        .subcommand(
            tls_protocol_command("add-tuic-tls", "添加 TUIC + TLS 节点", "udp")
                .arg(
                    Arg::new("uuid")
                        .long("uuid")
                        .value_name("UUID")
                        .help("指定 UUID，默认自动生成"),
                )
                .arg(
                    Arg::new("password")
                        .long("password")
                        .value_name("PASSWORD")
                        .help("指定密码，默认自动生成"),
                ),
        )
        .subcommand(
            protocol_command("add-shadowsocks", "添加 Shadowsocks 节点", "tcp+udp")
                .arg(
                    Arg::new("method")
                        .long("method")
                        .value_name("METHOD")
                        .help("加密方法"),
                )
                .arg(
                    Arg::new("password")
                        .long("password")
                        .value_name("PASSWORD")
                        .help("指定密码，默认自动生成"),
                )
                .arg(
                    Arg::new("show-secrets")
                        .long("show-secrets")
                        .action(ArgAction::SetTrue)
                        .help("输出敏感链接与客户端 JSON"),
                ),
        )
        .subcommand(
            Command::new("list-nodes")
                .about("列出当前所有服务端节点")
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .value_name("N")
                        .help("返回的节点数量上限"),
                ),
        )
        .subcommand(
            Command::new("show-links")
                .about("查看节点链接或客户端 JSON")
                .arg(
                    Arg::new("id")
                        .long("id")
                        .value_name("ID")
                        .help("节点 tag；不提供时展示全部"),
                )
                .arg(
                    Arg::new("show-secrets")
                        .long("show-secrets")
                        .action(ArgAction::SetTrue)
                        .help("显示包含凭据的敏感信息"),
                ),
        )
        .subcommand(
            Command::new("show-config")
                .about("查看配置摘要或完整配置")
                .arg(
                    Arg::new("sensitive")
                        .long("sensitive")
                        .action(ArgAction::SetTrue)
                        .help("显示完整配置 JSON，包含敏感字段"),
                ),
        )
        .subcommand(Command::new("check-config").about("检查 sing-box 配置"))
        .subcommand(
            Command::new("logs")
                .about("查看 sing-box 服务日志")
                .arg(
                    Arg::new("lines")
                        .long("lines")
                        .value_name("N")
                        .help("显示最近 N 行，默认 100"),
                )
                .arg(
                    Arg::new("follow")
                        .long("follow")
                        .action(ArgAction::SetTrue)
                        .help("持续跟随日志，仅适用于交互环境"),
                ),
        )
        .subcommand(
            Command::new("remove-node")
                .about("删除某个服务端节点")
                .arg(
                    Arg::new("id")
                        .long("id")
                        .value_name("ID")
                        .help("要删除的节点 ID")
                        .required(true),
                )
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认，直接删除"),
                ),
        )
        .subcommand(Command::new("status").about("查看 sing-box 服务状态"))
        .subcommand(Command::new("start").about("启动 sing-box 服务"))
        .subcommand(Command::new("stop").about("停止 sing-box 服务"))
        .subcommand(Command::new("restart").about("重启 sing-box 服务"))
}

pub fn handle_singbox(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    match matches.subcommand() {
        Some(("install", sub)) => install_singbox(sub, format, no_input),
        Some(("add-node", sub)) => add_node(sub, format, no_input),
        Some(("add-vless-reality", sub)) => add_vless_reality(sub, format, no_input),
        Some(("add-trojan-tls", sub)) => add_trojan_tls(sub, format, no_input),
        Some(("add-hysteria2-tls", sub)) => add_hysteria2_tls(sub, format, no_input),
        Some(("add-tuic-tls", sub)) => add_tuic_tls(sub, format, no_input),
        Some(("add-shadowsocks", sub)) => add_shadowsocks(sub, format, no_input),
        Some(("list-nodes", sub)) => list_nodes(sub, format),
        Some(("show-links", sub)) => show_links(sub, format, no_input),
        Some(("show-config", sub)) => show_config(sub, format),
        Some(("check-config", _)) => check_config(format),
        Some(("logs", sub)) => show_logs(sub, format, no_input),
        Some(("remove-node", sub)) => remove_node(sub, format, no_input),
        Some(("status", _)) => service_action("status", format),
        Some(("start", _)) => service_action("start", format),
        Some(("stop", _)) => service_action("stop", format),
        Some(("restart", _)) => service_action("restart", format),
        _ => Err(CliError::new("未知子命令")),
    }
}

fn install_singbox(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let version = matches.get_one::<String>("version").cloned();
    let sha256 = matches.get_one::<String>("sha256").cloned();
    let dry_run = matches.get_flag("dry-run");
    let confirm_flag = matches.get_flag("confirm");
    let interactive = is_interactive(no_input);
    let mut version = version;
    if interactive && version.is_none() {
        let input: String = Input::new()
            .with_prompt("请输入要安装的 sing-box 版本 (留空为最新稳定版)")
            .allow_empty(true)
            .interact_text()
            .map_err(|e| CliError::new(format!("读取版本失败: {}", e)))?;
        if !input.trim().is_empty() {
            version = Some(input);
        }
    }
    require_confirmation(
        "确认开始安装或更新 sing-box？",
        interactive,
        dry_run,
        confirm_flag,
    )?;

    let arch = detect_arch().ok_or_else(|| CliError::new("无法识别当前 CPU 架构"))?;

    if dry_run {
        let version_preview = version
            .as_deref()
            .map(normalize_version)
            .transpose()?
            .unwrap_or_else(|| "latest-stable".to_string());
        emit_success(
            format,
            json!({
                "dry_run": true,
                "version": version_preview,
                "arch": arch,
                "config": SINGBOX_CONFIG_PATH
            }),
            &OperationReport::default(),
            Some(vec![crumb("执行安装", "vps-cli singbox install --confirm")]),
        );
        return Ok(());
    }

    let release = resolve_singbox_release(version.as_deref(), arch)?;
    let version_str = release.version.clone();
    let download_url = release.download_url.clone();
    let tmp_dir = PathBuf::from(format!("/tmp/singbox-install-{}", version_str));
    let _ = fs::remove_dir_all(&tmp_dir);
    let _ = fs::create_dir_all(&tmp_dir);
    let tar_path = tmp_dir.join("sing-box.tar.gz");
    download_file(&download_url, &tar_path)
        .map_err(|e| CliError::new(format!("下载失败: {}", e)))?;
    if let Some(check) = sha256 {
        let computed = compute_sha256(&tar_path)
            .map_err(|e| CliError::new(format!("计算 sha256 失败: {}", e)))?;
        if computed.to_lowercase() != check.to_lowercase() {
            return Err(CliError::new(format!(
                "SHA256 校验失败: 期待 {}, 计算得 {}",
                check, computed
            )));
        }
    }
    extract_tar(&tar_path, &tmp_dir).map_err(|e| CliError::new(format!("解压失败: {}", e)))?;
    let extracted_subdir = find_extracted_singbox_dir(&tmp_dir)?;
    install_binary(&extracted_subdir)
        .map_err(|e| CliError::new(format!("安装二进制失败: {}", e)))?;
    create_system_user().map_err(|e| CliError::new(format!("创建系统用户失败: {}", e)))?;
    create_dirs().map_err(|e| CliError::new(format!("创建目录失败: {}", e)))?;
    create_service_file().map_err(|e| CliError::new(format!("创建 systemd 服务失败: {}", e)))?;
    reload_systemd().map_err(|e| CliError::new(format!("重载 systemd 失败: {}", e)))?;
    enable_service().map_err(|e| CliError::new(format!("启用服务失败: {}", e)))?;
    if read_singbox_config().is_none() {
        write_singbox_config(&default_singbox_config())
            .map_err(|e| CliError::new(format!("写入默认配置失败: {}", e)))?;
    }

    let mut report = OperationReport::default();
    report.changed_files.push(SINGBOX_BINARY_PATH.into());
    report.changed_files.push(SINGBOX_SERVICE_FILE.into());
    report.changed_files.push(SINGBOX_CONFIG_PATH.into());
    emit_success(
        format,
        json!({
            "installed": true,
            "version": version_str,
            "tag": release.tag,
            "arch": arch,
            "asset": release.asset_name,
            "url": download_url
        }),
        &report,
        Some(vec![
            crumb("添加节点", "vps-cli singbox add-vless-reality --confirm"),
            crumb("检查配置", "vps-cli singbox check-config"),
        ]),
    );
    Ok(())
}

fn add_node(matches: &ArgMatches, format: OutputFormat, no_input: bool) -> Result<(), CliError> {
    let node_type = matches.get_one::<String>("type").unwrap().clone();
    let config_file = matches.get_one::<String>("config-file").unwrap().clone();
    let dry_run = matches.get_flag("dry-run");
    let confirm_flag = matches.get_flag("confirm");
    let interactive = is_interactive(no_input);
    require_confirmation(
        &format!("确认导入 {} 节点？", node_type),
        interactive,
        dry_run,
        confirm_flag,
    )?;

    let path = PathBuf::from(&config_file);
    if !path.exists() {
        return Err(CliError::new(format!("配置文件不存在: {}", config_file)));
    }

    if dry_run {
        emit_success(
            format,
            json!({"dry_run": true, "type": node_type, "config_file": config_file }),
            &OperationReport::default(),
            Some(vec![crumb(
                "执行导入",
                "vps-cli singbox add-node --confirm --type <type> --config-file <file>",
            )]),
        );
        return Ok(());
    }

    let node_value: JsonValue = {
        let file =
            fs::File::open(&path).map_err(|e| CliError::new(format!("读取配置文件失败: {}", e)))?;
        serde_json::from_reader(file)
            .map_err(|e| CliError::new(format!("解析配置文件失败: {}", e)))?
    };
    let mut inbounds = read_singbox_config().unwrap_or_else(default_singbox_config);
    ensure_inbounds_array(&mut inbounds);
    let base_len = inbounds
        .get("inbounds")
        .and_then(|v| v.as_array())
        .map(|v| v.len())
        .unwrap_or(0);
    let target = inbounds
        .get_mut("inbounds")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| CliError::new("配置中的 inbounds 不是数组"))?;
    let mut metas = read_node_meta()?;
    let mut added = Vec::new();
    if let JsonValue::Array(arr) = node_value {
        for (idx, mut n) in arr.into_iter().enumerate() {
            if let JsonValue::Object(ref mut map) = n {
                if !map.contains_key("tag") {
                    map.insert(
                        "tag".into(),
                        JsonValue::String(format!("{}-{}", node_type, base_len + idx + 1)),
                    );
                }
            }
            let meta = build_generic_meta(&n)?;
            target.push(n);
            metas.push(meta.clone());
            added.push(meta.tag);
        }
    } else if let JsonValue::Object(mut map) = node_value {
        if !map.contains_key("tag") {
            map.insert(
                "tag".into(),
                JsonValue::String(format!("{}-{}", node_type, base_len + 1)),
            );
        }
        let inbound = JsonValue::Object(map);
        let meta = build_generic_meta(&inbound)?;
        target.push(inbound);
        metas.push(meta.clone());
        added.push(meta.tag);
    } else {
        return Err(CliError::new(
            "节点配置文件格式不正确，应为 JSON 对象或数组，且表示服务端 inbounds",
        ));
    }
    let report = apply_config_and_meta(&inbounds, &metas)?;
    emit_success(
        format,
        json!({"type": node_type, "added": added}),
        &report,
        Some(vec![
            crumb("查看节点", "vps-cli singbox list-nodes"),
            crumb("查看配置", "vps-cli singbox check-config"),
        ]),
    );
    Ok(())
}

fn list_nodes(matches: &ArgMatches, format: OutputFormat) -> Result<(), CliError> {
    let limit = matches
        .get_one::<String>("limit")
        .and_then(|v| v.parse::<usize>().ok());
    let config = read_singbox_config().unwrap_or_else(default_singbox_config);
    let mut nodes: Vec<NodeSummary> = Vec::new();
    if let Some(inbounds) = config.get("inbounds").and_then(|v| v.as_array()) {
        for (idx, inbound) in inbounds.iter().enumerate() {
            nodes.push(NodeSummary {
                id: inbound
                    .get("tag")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("{}", idx + 1)),
                node_type: inbound
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                port: inbound
                    .get("listen_port")
                    .and_then(|v| v.as_u64())
                    .unwrap_or_default() as u16,
                network: infer_network(inbound),
            });
        }
    }
    let total = nodes.len();
    if let Some(l) = limit {
        nodes.truncate(l);
    }
    match format {
        OutputFormat::Json => {
            let breadcrumbs = if limit.is_none() && total > nodes.len() {
                Some(vec![crumb(
                    "限制输出",
                    "vps-cli singbox list-nodes --limit 10 --json",
                )])
            } else {
                None
            };
            emit_success(
                format,
                serde_json::to_value(nodes).unwrap(),
                &OperationReport::default(),
                breadcrumbs,
            );
        }
        OutputFormat::Plain => {
            for node in &nodes {
                println!(
                    "{}\t{}\t{}\t{}",
                    node.id, node.node_type, node.port, node.network
                );
            }
            if total > nodes.len() {
                eprintln!("显示 {} 条记录，使用 --limit 缩小输出", nodes.len());
            }
        }
        OutputFormat::Human => {
            println!("节点列表:");
            for node in &nodes {
                println!(
                    "- 标签: {}，类型: {}，端口: {}，协议: {}",
                    node.id, node.node_type, node.port, node.network
                );
            }
            if total > nodes.len() {
                println!("\n共 {} 条记录，建议使用 --limit 缩小输出。", nodes.len());
            }
        }
    }
    Ok(())
}

fn remove_node(matches: &ArgMatches, format: OutputFormat, no_input: bool) -> Result<(), CliError> {
    let id = matches.get_one::<String>("id").unwrap().clone();
    let confirm_flag = matches.get_flag("confirm");
    let interactive = is_interactive(no_input);
    require_confirmation(
        &format!("确认删除节点 {}？", id),
        interactive,
        false,
        confirm_flag,
    )?;
    let mut config = read_singbox_config().unwrap_or_else(default_singbox_config);
    let mut removed = false;
    if let Some(inbounds) = config.get_mut("inbounds").and_then(|v| v.as_array_mut()) {
        if id.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(idx) = id.parse::<usize>() {
                if idx > 0 && idx <= inbounds.len() {
                    inbounds.remove(idx - 1);
                    removed = true;
                }
            }
        } else {
            if let Some(pos) = inbounds
                .iter()
                .position(|n| n.get("tag").and_then(|v| v.as_str()) == Some(id.as_str()))
            {
                inbounds.remove(pos);
                removed = true;
            }
        }
    }
    if !removed {
        return Err(CliError::new(format!("未找到节点 {}", id)));
    }
    let mut metas = read_node_meta()?;
    metas.retain(|item| item.tag != id);
    let report = apply_config_and_meta(&config, &metas)?;
    emit_success(
        format,
        json!({"removed": id}),
        &report,
        Some(vec![crumb("查看节点", "vps-cli singbox list-nodes --json")]),
    );
    Ok(())
}

fn service_action(action: &str, format: OutputFormat) -> Result<(), CliError> {
    let service_name = "sing-box";
    let result = match action {
        "status" => SysCmd::new("systemctl")
            .args(&["is-active", service_name])
            .output(),
        "start" => SysCmd::new("systemctl")
            .args(&["start", service_name])
            .output(),
        "stop" => SysCmd::new("systemctl")
            .args(&["stop", service_name])
            .output(),
        "restart" => SysCmd::new("systemctl")
            .args(&["restart", service_name])
            .output(),
        _ => return Err(CliError::new("未知操作")),
    };
    match result {
        Ok(output) => {
            let success = output.status.success();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            match action {
                "status" => {
                    let status_str = if success {
                        String::from_utf8_lossy(&output.stdout).trim().to_string()
                    } else {
                        "unknown".to_string()
                    };
                    let mut report = OperationReport::default();
                    if !stderr.is_empty() {
                        report.warnings.push(stderr);
                    }
                    emit_success(
                        format,
                        json!({"status": status_str}),
                        &report,
                        Some(vec![crumb("重启服务", "vps-cli singbox restart")]),
                    );
                }
                "start" => {
                    if !success {
                        return Err(
                            CliError::new("启动 sing-box 服务失败").with_warnings(vec![stderr])
                        );
                    }
                    emit_success(
                        format,
                        json!({"started": true}),
                        &OperationReport::default(),
                        Some(vec![crumb("查看状态", "vps-cli singbox status")]),
                    );
                }
                "stop" => {
                    if !success {
                        return Err(
                            CliError::new("停止 sing-box 服务失败").with_warnings(vec![stderr])
                        );
                    }
                    emit_success(
                        format,
                        json!({"stopped": true}),
                        &OperationReport::default(),
                        Some(vec![crumb("重新启动", "vps-cli singbox start")]),
                    );
                }
                "restart" => {
                    if !success {
                        return Err(
                            CliError::new("重启 sing-box 服务失败").with_warnings(vec![stderr])
                        );
                    }
                    emit_success(
                        format,
                        json!({"restarted": true}),
                        &OperationReport::default(),
                        Some(vec![crumb("查看状态", "vps-cli singbox status")]),
                    );
                }
                _ => {}
            }
        }
        Err(e) => return Err(CliError::new(format!("执行 systemctl 命令失败: {}", e))),
    }
    Ok(())
}

fn detect_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" | "amd64" => Some("amd64"),
        "aarch64" => Some("arm64"),
        "armv7" => Some("armv7"),
        "i386" | "i686" => Some("386"),
        _ => None,
    }
}
fn read_singbox_config() -> Option<JsonValue> {
    let path = Path::new(SINGBOX_CONFIG_PATH);
    if !path.exists() {
        return None;
    }
    let file = File::open(path).ok()?;
    let json: Result<JsonValue, _> = serde_json::from_reader(file);
    json.ok()
}

fn write_singbox_config(config: &JsonValue) -> Result<(), io::Error> {
    let path = Path::new(SINGBOX_CONFIG_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    let data = serde_json::to_vec_pretty(config)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    file.write_all(&data)?;
    Ok(())
}

fn read_node_meta() -> Result<Vec<NodeMeta>, CliError> {
    let path = Path::new(SINGBOX_META_PATH);
    if !path.exists() {
        return Ok(vec![]);
    }
    let file = File::open(path).map_err(|e| CliError::new(format!("读取节点元数据失败: {}", e)))?;
    serde_json::from_reader(file).map_err(|e| CliError::new(format!("解析节点元数据失败: {}", e)))
}

fn write_node_meta(meta: &[NodeMeta]) -> Result<(), CliError> {
    if let Some(parent) = Path::new(SINGBOX_META_PATH).parent() {
        ensure_dir(parent)?;
    }
    let data = serde_json::to_vec_pretty(meta)
        .map_err(|e| CliError::new(format!("序列化节点元数据失败: {}", e)))?;
    fs::write(SINGBOX_META_PATH, data)
        .map_err(|e| CliError::new(format!("写入节点元数据失败: {}", e)))
}

fn ensure_inbounds_array(config: &mut JsonValue) {
    if !config
        .get("inbounds")
        .map(|v| v.is_array())
        .unwrap_or(false)
    {
        config["inbounds"] = JsonValue::Array(vec![]);
    }
}

fn default_singbox_config() -> JsonValue {
    json!({
        "log": {"level": "info"},
        "inbounds": [],
        "outbounds": [
            {"type": "direct", "tag": "direct"},
            {"type": "block", "tag": "block"}
        ]
    })
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
        OutputFormat::Plain => {
            println!("{}", serde_json::to_string_pretty(&data).unwrap());
        }
        OutputFormat::Human => {
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

#[derive(Debug, Clone)]
struct SingboxReleaseSelection {
    tag: String,
    version: String,
    asset_name: String,
    download_url: String,
}

fn resolve_singbox_release(
    version: Option<&str>,
    arch: &str,
) -> Result<SingboxReleaseSelection, CliError> {
    let version = version.map(normalize_version).transpose()?;
    let api_url = match version.as_deref() {
        Some(v) => format!(
            "https://api.github.com/repos/SagerNet/sing-box/releases/tags/v{}",
            v
        ),
        None => "https://api.github.com/repos/SagerNet/sing-box/releases/latest".to_string(),
    };
    let release = fetch_github_release(&api_url)?;
    let normalized = normalize_version(&release.tag_name)?;
    let asset_name = format!("sing-box-{}-linux-{}.tar.gz", normalized, arch);
    let asset = release
        .assets
        .iter()
        .find(|item| item.name == asset_name)
        .ok_or_else(|| {
            CliError::new(format!(
                "未在 sing-box release {} 中找到适用于 {} 的安装包 {}",
                release.tag_name, arch, asset_name
            ))
        })?;
    Ok(SingboxReleaseSelection {
        tag: release.tag_name,
        version: normalized,
        asset_name,
        download_url: asset.browser_download_url.clone(),
    })
}

fn fetch_github_release(url: &str) -> Result<GitHubRelease, CliError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("vps-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| CliError::new(format!("创建 GitHub 请求失败: {}", e)))?;
    let response = client
        .get(url)
        .send()
        .map_err(|e| CliError::new(format!("请求 sing-box release 信息失败: {}", e)))?;
    if !response.status().is_success() {
        return Err(CliError::new(format!(
            "请求 sing-box release 信息失败：HTTP {}",
            response.status()
        )));
    }
    let body = response
        .text()
        .map_err(|e| CliError::new(format!("读取 sing-box release 响应失败: {}", e)))?;
    serde_json::from_str::<GitHubRelease>(&body)
        .map_err(|e| CliError::new(format!("解析 sing-box release 信息失败: {}", e)))
}

fn normalize_version(value: &str) -> Result<String, CliError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(CliError::new("sing-box 版本不能为空"));
    }
    Ok(trimmed.trim_start_matches('v').to_string())
}

fn find_extracted_singbox_dir(tmp_dir: &Path) -> Result<PathBuf, CliError> {
    let entries =
        fs::read_dir(tmp_dir).map_err(|e| CliError::new(format!("读取解压目录失败: {}", e)))?;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().into_string().unwrap_or_default();
            if name.starts_with("sing-box-") {
                return Ok(path);
            }
        }
    }
    Err(CliError::new("未找到解压后的 sing-box 目录"))
}

fn crumb(action: &str, cmd: &str) -> Breadcrumb {
    Breadcrumb {
        action: action.into(),
        cmd: cmd.into(),
    }
}

fn download_file(url: &str, dest: &Path) -> Result<(), io::Error> {
    let response = reqwest::blocking::get(url)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    if !response.status().is_success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("下载失败: HTTP {}", response.status()),
        ));
    }
    let mut out = fs::File::create(dest)?;
    let mut content = io::BufReader::new(response);
    io::copy(&mut content, &mut out)?;
    Ok(())
}

fn compute_sha256(path: &Path) -> Result<String, io::Error> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn extract_tar(tar_path: &Path, dest_dir: &Path) -> Result<(), io::Error> {
    let tar_gz = fs::File::open(tar_path)?;
    let decompressed = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(decompressed);
    archive.unpack(dest_dir)?;
    Ok(())
}

fn install_binary(extracted_dir: &Path) -> Result<(), io::Error> {
    let bin_path = extracted_dir.join("sing-box");
    if !bin_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "未找到 sing-box 可执行文件",
        ));
    }
    fs::create_dir_all("/usr/local/bin")?;
    fs::copy(&bin_path, SINGBOX_BINARY_PATH)?;
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(SINGBOX_BINARY_PATH)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(SINGBOX_BINARY_PATH, perms)?;
    Ok(())
}

fn create_system_user() -> Result<(), io::Error> {
    let status = SysCmd::new("id").arg("-u").arg("sing-box").status();
    if let Ok(st) = status {
        if st.success() {
            return Ok(());
        }
    }
    let status = SysCmd::new("useradd")
        .args(&[
            "--system",
            "--no-create-home",
            "--shell",
            "/usr/sbin/nologin",
            "sing-box",
        ])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "创建用户失败"))
    }
}

fn create_dirs() -> Result<(), io::Error> {
    fs::create_dir_all("/etc/sing-box")?;
    fs::create_dir_all("/var/log/sing-box")?;
    Ok(())
}

fn create_service_file() -> Result<(), io::Error> {
    let service_content = r#"[Unit]
Description=sing-box service
After=network.target

[Service]
Type=simple
User=sing-box
ExecStart=/usr/local/bin/sing-box run -c /etc/sing-box/config.json
Restart=on-failure
LimitNOFILE=1048576

[Install]
WantedBy=multi-user.target
"#;
    fs::write(SINGBOX_SERVICE_FILE, service_content)?;
    Ok(())
}

fn reload_systemd() -> Result<(), io::Error> {
    let status = SysCmd::new("systemctl").arg("daemon-reload").status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "systemctl daemon-reload 失败",
        ))
    }
}

fn enable_service() -> Result<(), io::Error> {
    let status = SysCmd::new("systemctl")
        .args(&["enable", "--now", "sing-box"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "启动 sing-box 服务失败",
        ))
    }
}

fn protocol_command(name: &'static str, about: &'static str, network: &'static str) -> Command {
    Command::new(name)
        .about(about)
        .arg(
            Arg::new("server")
                .long("server")
                .value_name("HOST")
                .help("客户端访问使用的公网域名或 IP"),
        )
        .arg(
            Arg::new("port")
                .long("port")
                .value_name("PORT")
                .help("监听端口"),
        )
        .arg(
            Arg::new("tag")
                .long("tag")
                .value_name("TAG")
                .help("节点标签"),
        )
        .arg(
            Arg::new("listen")
                .long("listen")
                .value_name("ADDR")
                .help("监听地址，默认 ::"),
        )
        .arg(
            Arg::new("network")
                .long("network")
                .value_name("PROTO")
                .default_value(network)
                .hide(true),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .action(ArgAction::SetTrue)
                .help("仅预览生成结果，不实际写入"),
        )
        .arg(
            Arg::new("confirm")
                .long("confirm")
                .action(ArgAction::SetTrue)
                .help("跳过交互确认，直接写入"),
        )
}

fn tls_protocol_command(name: &'static str, about: &'static str, network: &'static str) -> Command {
    protocol_command(name, about, network)
        .arg(
            Arg::new("server-name")
                .long("server-name")
                .value_name("SNI")
                .help("TLS SNI / 证书域名；默认与 --server 相同"),
        )
        .arg(
            Arg::new("cert-path")
                .long("cert-path")
                .value_name("FILE")
                .help("证书路径"),
        )
        .arg(
            Arg::new("key-path")
                .long("key-path")
                .value_name("FILE")
                .help("私钥路径"),
        )
        .arg(
            Arg::new("self-signed")
                .long("self-signed")
                .action(ArgAction::SetTrue)
                .help("自动生成自签名证书"),
        )
        .arg(
            Arg::new("show-secrets")
                .long("show-secrets")
                .action(ArgAction::SetTrue)
                .help("输出敏感链接与客户端 JSON"),
        )
}

fn add_vless_reality(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let interactive = is_interactive(no_input);
    let dry_run = matches.get_flag("dry-run");
    let confirm = matches.get_flag("confirm");
    let show_secrets = matches.get_flag("show-secrets");
    let server = required_or_prompt(
        matches.get_one::<String>("server").cloned(),
        "请输入客户端使用的服务器公网域名/IP",
        interactive,
    )?;
    let port = parse_port(required_or_prompt(
        matches.get_one::<String>("port").cloned(),
        "请输入 TCP 监听端口",
        interactive,
    )?)?;
    let tag = matches
        .get_one::<String>("tag")
        .cloned()
        .unwrap_or_else(|| format!("vless-reality-{}", short_hex(4)));
    let server_name = resolve_server_name(
        matches.get_one::<String>("server-name").cloned(),
        &server,
        interactive,
        "连接地址是 IP，请输入 Reality 握手域名 / SNI",
        "当 --server 为 IP 时，VLESS Reality 必须显式提供 --server-name；如果你确实要使用 IP 作为 SNI，也请显式传入相同 IP。",
    )?;
    let uuid = matches
        .get_one::<String>("uuid")
        .cloned()
        .unwrap_or_else(new_uuid);
    let short_id = matches
        .get_one::<String>("short-id")
        .cloned()
        .unwrap_or_else(|| short_hex(4));
    let listen = matches
        .get_one::<String>("listen")
        .cloned()
        .unwrap_or_else(|| "::".into());
    let (private_key, public_key) = match (
        matches.get_one::<String>("private-key").cloned(),
        matches.get_one::<String>("public-key").cloned(),
    ) {
        (Some(privk), Some(pubk)) => (privk, pubk),
        (None, None) => generate_reality_keypair()?,
        _ => {
            return Err(CliError::new(
                "若手动指定 Reality 密钥，请同时提供 --private-key 和 --public-key",
            ))
        }
    };
    let inbound = json!({
        "type": "vless",
        "tag": tag,
        "listen": listen,
        "listen_port": port,
        "users": [{"name": "default", "uuid": uuid, "flow": "xtls-rprx-vision"}],
        "tls": {
            "enabled": true,
            "server_name": server_name,
            "reality": {
                "enabled": true,
                "handshake": {"server": server_name, "server_port": 443},
                "private_key": private_key,
                "short_id": [short_id]
            }
        }
    });
    let link = format!(
        "vless://{}@{}:{}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={}&fp=chrome&pbk={}&sid={}&type=tcp#{}",
        uri_component(&uuid),
        format_uri_host(&server),
        port,
        uri_component(&server_name),
        uri_component(&public_key),
        uri_component(&short_id),
        uri_fragment(&tag)
    );
    let client_json = json!({
        "type": "vless",
        "tag": tag,
        "server": server,
        "server_port": port,
        "uuid": uuid,
        "flow": "xtls-rprx-vision",
        "tls": {
            "enabled": true,
            "server_name": server_name,
            "utls": {"enabled": true, "fingerprint": "chrome"},
            "reality": {"enabled": true, "public_key": public_key, "short_id": short_id}
        }
    });
    commit_inbound(
        "vless-reality",
        inbound,
        NodeMeta {
            tag: tag.clone(),
            node_type: "vless-reality".into(),
            server: server.clone(),
            port,
            network: "tcp".into(),
            link: Some(link.clone()),
            client_json: Some(client_json.clone()),
            created_at: iso_now(),
        },
        dry_run,
        confirm,
        interactive,
        format,
        show_secrets,
    )
}

fn add_trojan_tls(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let interactive = is_interactive(no_input);
    let dry_run = matches.get_flag("dry-run");
    let confirm = matches.get_flag("confirm");
    let show_secrets = matches.get_flag("show-secrets");
    let server = required_or_prompt(
        matches.get_one::<String>("server").cloned(),
        "请输入客户端使用的服务器公网域名",
        interactive,
    )?;
    let port = parse_port(required_or_prompt(
        matches.get_one::<String>("port").cloned(),
        "请输入 TCP 监听端口",
        interactive,
    )?)?;
    let tag = matches
        .get_one::<String>("tag")
        .cloned()
        .unwrap_or_else(|| format!("trojan-{}", short_hex(4)));
    let server_name = resolve_server_name(
        matches.get_one::<String>("server-name").cloned(),
        &server,
        interactive,
        "连接地址是 IP，请输入 TLS SNI / 证书域名",
        "当 --server 为 IP 时，请显式提供 --server-name；如果你确实要使用 IP 作为 SNI，也请显式传入相同 IP。",
    )?;
    let password = matches
        .get_one::<String>("password")
        .cloned()
        .unwrap_or_else(|| random_b64url(24));
    let (cert_path, key_path, insecure) = resolve_tls_material(matches, interactive, &server_name)?;
    let listen = matches
        .get_one::<String>("listen")
        .cloned()
        .unwrap_or_else(|| "::".into());
    let inbound = json!({
        "type": "trojan",
        "tag": tag,
        "listen": listen,
        "listen_port": port,
        "users": [{"name": "default", "password": password}],
        "tls": {"enabled": true, "server_name": server_name, "certificate_path": cert_path, "key_path": key_path}
    });
    let link = format!(
        "trojan://{}@{}:{}?security=tls&sni={}{}#{}",
        uri_component(&password),
        format_uri_host(&server),
        port,
        uri_component(&server_name),
        if insecure { "&allowInsecure=1" } else { "" },
        uri_fragment(&tag)
    );
    let client_json = json!({
        "type":"trojan","tag":tag,"server":server,"server_port":port,"password":password,
        "tls":{"enabled":true,"server_name":server_name,"insecure":insecure}
    });
    commit_inbound(
        "trojan-tls",
        inbound,
        NodeMeta {
            tag: tag.clone(),
            node_type: "trojan-tls".into(),
            server: server.clone(),
            port,
            network: "tcp".into(),
            link: Some(link),
            client_json: Some(client_json),
            created_at: iso_now(),
        },
        dry_run,
        confirm,
        interactive,
        format,
        show_secrets,
    )
}

fn add_hysteria2_tls(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let interactive = is_interactive(no_input);
    let dry_run = matches.get_flag("dry-run");
    let confirm = matches.get_flag("confirm");
    let show_secrets = matches.get_flag("show-secrets");
    let server = required_or_prompt(
        matches.get_one::<String>("server").cloned(),
        "请输入客户端使用的服务器公网域名",
        interactive,
    )?;
    let port = parse_port(required_or_prompt(
        matches.get_one::<String>("port").cloned(),
        "请输入 UDP 监听端口",
        interactive,
    )?)?;
    let tag = matches
        .get_one::<String>("tag")
        .cloned()
        .unwrap_or_else(|| format!("hy2-{}", short_hex(4)));
    let server_name = resolve_server_name(
        matches.get_one::<String>("server-name").cloned(),
        &server,
        interactive,
        "连接地址是 IP，请输入 TLS SNI / 证书域名",
        "当 --server 为 IP 时，请显式提供 --server-name；如果你确实要使用 IP 作为 SNI，也请显式传入相同 IP。",
    )?;
    let password = matches
        .get_one::<String>("password")
        .cloned()
        .unwrap_or_else(|| random_b64url(24));
    let obfs = matches
        .get_one::<String>("obfs-password")
        .cloned()
        .unwrap_or_else(|| random_b64url(18));
    let (cert_path, key_path, insecure) = resolve_tls_material(matches, interactive, &server_name)?;
    let listen = matches
        .get_one::<String>("listen")
        .cloned()
        .unwrap_or_else(|| "::".into());
    let inbound = json!({
        "type":"hysteria2","tag":tag,"listen":listen,"listen_port":port,
        "users":[{"name":"default","password":password}],
        "obfs":{"type":"salamander","password":obfs},
        "ignore_client_bandwidth":false,
        "tls":{"enabled":true,"server_name":server_name,"certificate_path":cert_path,"key_path":key_path}
    });
    let link = format!(
        "hysteria2://{}@{}:{}?sni={}&obfs=salamander&obfs-password={}{}#{}",
        uri_component(&password),
        format_uri_host(&server),
        port,
        uri_component(&server_name),
        uri_component(&obfs),
        if insecure { "&insecure=1" } else { "" },
        uri_fragment(&tag)
    );
    let client_json = json!({
        "type":"hysteria2","tag":tag,"server":server,"server_port":port,"password":password,
        "obfs":{"type":"salamander","password":obfs},
        "tls":{"enabled":true,"server_name":server_name,"insecure":insecure}
    });
    commit_inbound(
        "hysteria2-tls",
        inbound,
        NodeMeta {
            tag: tag.clone(),
            node_type: "hysteria2-tls".into(),
            server: server.clone(),
            port,
            network: "udp".into(),
            link: Some(link),
            client_json: Some(client_json),
            created_at: iso_now(),
        },
        dry_run,
        confirm,
        interactive,
        format,
        show_secrets,
    )
}

fn add_tuic_tls(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let interactive = is_interactive(no_input);
    let dry_run = matches.get_flag("dry-run");
    let confirm = matches.get_flag("confirm");
    let show_secrets = matches.get_flag("show-secrets");
    let server = required_or_prompt(
        matches.get_one::<String>("server").cloned(),
        "请输入客户端使用的服务器公网域名",
        interactive,
    )?;
    let port = parse_port(required_or_prompt(
        matches.get_one::<String>("port").cloned(),
        "请输入 UDP 监听端口",
        interactive,
    )?)?;
    let tag = matches
        .get_one::<String>("tag")
        .cloned()
        .unwrap_or_else(|| format!("tuic-{}", short_hex(4)));
    let server_name = resolve_server_name(
        matches.get_one::<String>("server-name").cloned(),
        &server,
        interactive,
        "连接地址是 IP，请输入 TLS SNI / 证书域名",
        "当 --server 为 IP 时，请显式提供 --server-name；如果你确实要使用 IP 作为 SNI，也请显式传入相同 IP。",
    )?;
    let uuid = matches
        .get_one::<String>("uuid")
        .cloned()
        .unwrap_or_else(new_uuid);
    let password = matches
        .get_one::<String>("password")
        .cloned()
        .unwrap_or_else(|| random_b64url(20));
    let (cert_path, key_path, insecure) = resolve_tls_material(matches, interactive, &server_name)?;
    let listen = matches
        .get_one::<String>("listen")
        .cloned()
        .unwrap_or_else(|| "::".into());
    let inbound = json!({
        "type":"tuic","tag":tag,"listen":listen,"listen_port":port,
        "users":[{"name":"default","uuid":uuid,"password":password}],
        "congestion_control":"bbr","auth_timeout":"3s","zero_rtt_handshake":false,"heartbeat":"10s",
        "tls":{"enabled":true,"server_name":server_name,"alpn":["h3"],"certificate_path":cert_path,"key_path":key_path}
    });
    let link = format!(
        "tuic://{}:{}@{}:{}?congestion_control=bbr&udp_relay_mode=native&alpn=h3&sni={}{}#{}",
        uri_component(&uuid),
        uri_component(&password),
        format_uri_host(&server),
        port,
        uri_component(&server_name),
        if insecure { "&allow_insecure=1" } else { "" },
        uri_fragment(&tag)
    );
    let client_json = json!({
        "type":"tuic","tag":tag,"server":server,"server_port":port,"uuid":uuid,"password":password,
        "congestion_control":"bbr","udp_relay_mode":"native","zero_rtt_handshake":false,"heartbeat":"10s",
        "tls":{"enabled":true,"server_name":server_name,"alpn":["h3"],"insecure":insecure}
    });
    commit_inbound(
        "tuic-tls",
        inbound,
        NodeMeta {
            tag: tag.clone(),
            node_type: "tuic-tls".into(),
            server: server.clone(),
            port,
            network: "udp".into(),
            link: Some(link),
            client_json: Some(client_json),
            created_at: iso_now(),
        },
        dry_run,
        confirm,
        interactive,
        format,
        show_secrets,
    )
}

fn add_shadowsocks(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let interactive = is_interactive(no_input);
    let dry_run = matches.get_flag("dry-run");
    let confirm = matches.get_flag("confirm");
    let show_secrets = matches.get_flag("show-secrets");
    let server = required_or_prompt(
        matches.get_one::<String>("server").cloned(),
        "请输入客户端使用的服务器公网域名/IP",
        interactive,
    )?;
    let port = parse_port(required_or_prompt(
        matches.get_one::<String>("port").cloned(),
        "请输入 TCP/UDP 监听端口",
        interactive,
    )?)?;
    let tag = matches
        .get_one::<String>("tag")
        .cloned()
        .unwrap_or_else(|| format!("ss-{}", short_hex(4)));
    let method = match matches.get_one::<String>("method").cloned() {
        Some(m) => validate_ss_method(&m)?,
        None if interactive => {
            let methods = ss_methods();
            let idx = Select::new()
                .with_prompt("选择 Shadowsocks 加密方法")
                .items(&methods)
                .default(0)
                .interact()
                .map_err(|e| CliError::new(format!("读取选择失败: {}", e)))?;
            methods[idx].to_string()
        }
        None => ss_methods()[0].to_string(),
    };
    let password = matches
        .get_one::<String>("password")
        .cloned()
        .unwrap_or_else(|| ss_password_for_method(&method));
    let listen = matches
        .get_one::<String>("listen")
        .cloned()
        .unwrap_or_else(|| "::".into());
    let inbound = json!({
        "type":"shadowsocks","tag":tag,"listen":listen,"listen_port":port,"method":method,"password":password
    });
    let link = format!(
        "ss://{}@{}:{}#{}",
        ss_userinfo(&method, &password),
        format_uri_host(&server),
        port,
        uri_fragment(&tag)
    );
    let client_json = json!({
        "type":"shadowsocks","tag":tag,"server":server,"server_port":port,"method":method,"password":password
    });
    commit_inbound(
        "shadowsocks",
        inbound,
        NodeMeta {
            tag: tag.clone(),
            node_type: "shadowsocks".into(),
            server: server.clone(),
            port,
            network: "tcp+udp".into(),
            link: Some(link),
            client_json: Some(client_json),
            created_at: iso_now(),
        },
        dry_run,
        confirm,
        interactive,
        format,
        show_secrets,
    )
}

fn commit_inbound(
    node_type: &str,
    inbound: JsonValue,
    meta: NodeMeta,
    dry_run: bool,
    confirm: bool,
    interactive: bool,
    format: OutputFormat,
    show_secrets: bool,
) -> Result<(), CliError> {
    require_confirmation(
        &format!("确认添加 {} 节点？", node_type),
        interactive,
        dry_run,
        confirm,
    )?;
    if dry_run {
        let mut report = OperationReport::default();
        if show_secrets {
            report.sensitive = Some(true);
        }
        emit_success(
            format,
            json!({"dry_run": true, "tag": meta.tag, "type": meta.node_type, "inbound": inbound, "link": if show_secrets { meta.link.clone() } else { None::<String> }, "client_json": if show_secrets { meta.client_json.clone() } else { None::<JsonValue> }}),
            &report,
            Some(vec![crumb(
                "执行写入",
                &format!("vps-cli singbox {} --confirm", node_type),
            )]),
        );
        return Ok(());
    }

    let mut config = read_singbox_config().unwrap_or_else(default_singbox_config);
    ensure_inbounds_array(&mut config);
    let inbounds = config
        .get_mut("inbounds")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| CliError::new("配置中的 inbounds 不是数组"))?;
    if inbounds
        .iter()
        .any(|item| item.get("tag").and_then(|v| v.as_str()) == Some(meta.tag.as_str()))
    {
        return Err(CliError::new(format!("节点标签已存在: {}", meta.tag)));
    }
    if inbounds
        .iter()
        .any(|item| item.get("listen_port").and_then(|v| v.as_u64()) == Some(meta.port as u64))
        || is_port_in_use(meta.port, &meta.network)
    {
        return Err(CliError::new(format!(
            "端口 {} 已被占用或已存在于配置中",
            meta.port
        )));
    }
    inbounds.push(inbound);
    let mut metas = read_node_meta()?;
    metas.push(meta.clone());
    let report = apply_config_and_meta(&config, &metas)?;
    let mut data = json!({"added": meta.tag, "type": meta.node_type, "port": meta.port, "network": meta.network});
    if show_secrets {
        data["link"] = serde_json::to_value(meta.link).unwrap();
        data["client_json"] = serde_json::to_value(meta.client_json).unwrap();
    }
    let mut report = report;
    if show_secrets {
        report.sensitive = Some(true);
        report
            .warnings
            .push("当前输出包含敏感凭据，不应贴入公开日志。".into());
    }
    emit_success(
        format,
        data,
        &report,
        Some(vec![
            crumb("查看节点", "vps-cli singbox list-nodes"),
            crumb("查看链接", "vps-cli singbox show-links --show-secrets"),
        ]),
    );
    Ok(())
}

fn apply_config_and_meta(
    config: &JsonValue,
    meta: &[NodeMeta],
) -> Result<OperationReport, CliError> {
    ensure_dir(Path::new(SINGBOX_BACKUP_DIR))?;
    let cfg_backup = backup_path(
        Path::new(SINGBOX_CONFIG_PATH),
        Path::new(SINGBOX_BACKUP_DIR),
        "config",
    )?;
    let meta_backup = backup_path(
        Path::new(SINGBOX_META_PATH),
        Path::new(SINGBOX_BACKUP_DIR),
        "meta",
    )?;
    write_singbox_config(config)
        .map_err(|e| CliError::new(format!("写入 sing-box 配置失败: {}", e)))?;
    if let Err(err) = write_node_meta(meta) {
        if let Some(path) = &cfg_backup {
            let _ = restore_backup(path, Path::new(SINGBOX_CONFIG_PATH));
        }
        return Err(err);
    }
    if let Err(err) = validate_current_config() {
        if let Some(path) = &cfg_backup {
            let _ = restore_backup(path, Path::new(SINGBOX_CONFIG_PATH));
        }
        if let Some(path) = &meta_backup {
            let _ = restore_backup(path, Path::new(SINGBOX_META_PATH));
        }
        return Err(err.with_warnings(vec!["配置校验失败，已尝试回滚最近一次修改。".into()]));
    }
    if let Err(err) = restart_singbox_service() {
        if let Some(path) = &cfg_backup {
            let _ = restore_backup(path, Path::new(SINGBOX_CONFIG_PATH));
        }
        if let Some(path) = &meta_backup {
            let _ = restore_backup(path, Path::new(SINGBOX_META_PATH));
        }
        let _ = restart_singbox_service();
        return Err(err.with_warnings(vec![
            "sing-box 服务重启失败，已尝试回滚最近一次修改。".into()
        ]));
    }
    let mut report = OperationReport::default();
    report.changed_files.push(SINGBOX_CONFIG_PATH.into());
    report.changed_files.push(SINGBOX_META_PATH.into());
    report
        .warnings
        .push("已自动重启 sing-box 服务，使新增节点立即生效。".into());
    append_backup(&mut report, &cfg_backup);
    append_backup(&mut report, &meta_backup);
    Ok(report)
}

fn validate_current_config() -> Result<(), CliError> {
    if Path::new(SINGBOX_BINARY_PATH).exists() {
        let output = SysCmd::new(SINGBOX_BINARY_PATH)
            .args(["check", "-c", SINGBOX_CONFIG_PATH])
            .output()
            .map_err(|e| CliError::new(format!("执行 sing-box check 失败: {}", e)))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(CliError::new(format!(
                "sing-box 配置检查失败: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    } else {
        read_singbox_config().ok_or_else(|| CliError::new("当前配置文件不存在或不是合法 JSON"))?;
        Ok(())
    }
}

fn restart_singbox_service() -> Result<(), CliError> {
    if !Path::new(SINGBOX_SERVICE_FILE).exists() {
        return Ok(());
    }
    let output = SysCmd::new("systemctl")
        .args(["restart", "sing-box"])
        .output()
        .map_err(|e| CliError::new(format!("重启 sing-box 服务失败: {}", e)))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CliError::new(format!(
            "重启 sing-box 服务失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn check_config(format: OutputFormat) -> Result<(), CliError> {
    validate_current_config()?;
    emit_success(
        format,
        json!({"valid": true, "config": SINGBOX_CONFIG_PATH}),
        &OperationReport::default(),
        Some(vec![
            crumb("查看配置", "vps-cli singbox show-config"),
            crumb("查看日志", "vps-cli singbox logs"),
        ]),
    );
    Ok(())
}

fn show_config(matches: &ArgMatches, format: OutputFormat) -> Result<(), CliError> {
    let config = read_singbox_config().ok_or_else(|| CliError::new("未找到 sing-box 配置文件"))?;
    if matches.get_flag("sensitive") {
        let mut report = OperationReport::default();
        report.sensitive = Some(true);
        report
            .warnings
            .push("完整配置包含密钥、证书路径或凭据，不应贴入公开日志。".into());
        emit_success(format, config, &report, None);
    } else {
        let summaries = list_node_summaries();
        emit_success(
            format,
            json!({
                "config": SINGBOX_CONFIG_PATH,
                "inbounds": summaries,
                "tips": ["如需查看完整配置，请显式传入 --sensitive"]
            }),
            &OperationReport::default(),
            Some(vec![crumb(
                "查看完整配置",
                "vps-cli singbox show-config --sensitive",
            )]),
        );
    }
    Ok(())
}

fn show_links(matches: &ArgMatches, format: OutputFormat, no_input: bool) -> Result<(), CliError> {
    let metas = read_node_meta()?;
    let id = matches.get_one::<String>("id").cloned();
    let show_secrets = matches.get_flag("show-secrets");
    if !show_secrets {
        let data: Vec<_> = metas
            .iter()
            .filter(|m| id.as_ref().map(|wanted| wanted == &m.tag).unwrap_or(true))
            .map(|m| json!({"tag": m.tag, "type": m.node_type, "server": m.server, "port": m.port, "has_link": m.link.is_some()}))
            .collect();
        emit_success(
            format,
            json!({"safe_view": true, "nodes": data, "tips": ["如需查看敏感链接，请显式传入 --show-secrets"]}),
            &OperationReport::default(),
            Some(vec![crumb(
                "查看敏感链接",
                "vps-cli singbox show-links --show-secrets",
            )]),
        );
        return Ok(());
    }
    let interactive = is_interactive(no_input);
    require_confirmation(
        "当前输出将包含敏感链接和客户端 JSON，确认继续？",
        interactive,
        false,
        show_secrets,
    )?;
    let data: Vec<_> = metas
        .iter()
        .filter(|m| id.as_ref().map(|wanted| wanted == &m.tag).unwrap_or(true))
        .map(|m| json!({"tag": m.tag, "type": m.node_type, "link": m.link, "client_json": m.client_json}))
        .collect();
    let mut report = OperationReport::default();
    report.sensitive = Some(true);
    report
        .warnings
        .push("当前输出包含敏感凭据，不应贴入公开日志。".into());
    emit_success(format, json!(data), &report, None);
    Ok(())
}

fn show_logs(matches: &ArgMatches, format: OutputFormat, no_input: bool) -> Result<(), CliError> {
    let lines = matches
        .get_one::<String>("lines")
        .cloned()
        .unwrap_or_else(|| "100".into());
    let follow = matches.get_flag("follow");
    if follow && !is_interactive(no_input) {
        return Err(CliError::new("--follow 仅适用于交互环境"));
    }
    let mut cmd = SysCmd::new("journalctl");
    cmd.args(["-u", "sing-box", "-n", &lines, "--no-pager"]);
    if follow {
        cmd.arg("-f");
    }
    let output = cmd
        .output()
        .map_err(|e| CliError::new(format!("读取日志失败: {}", e)))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "读取日志失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    match format {
        OutputFormat::Json => emit_success(
            format,
            json!({"logs": text}),
            &OperationReport::default(),
            None,
        ),
        _ => print!("{}", text),
    }
    Ok(())
}

fn list_node_summaries() -> Vec<JsonValue> {
    let config = read_singbox_config().unwrap_or_else(default_singbox_config);
    config
        .get("inbounds")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    json!({
                        "tag": item.get("tag").and_then(|v| v.as_str()).unwrap_or(""),
                        "type": item.get("type").and_then(|v| v.as_str()).unwrap_or(""),
                        "port": item.get("listen_port").and_then(|v| v.as_u64()).unwrap_or_default(),
                        "network": infer_network(item),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn build_generic_meta(inbound: &JsonValue) -> Result<NodeMeta, CliError> {
    Ok(NodeMeta {
        tag: inbound
            .get("tag")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CliError::new("导入节点缺少 tag"))?
            .to_string(),
        node_type: inbound
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        server: inbound
            .get("listen")
            .and_then(|v| v.as_str())
            .unwrap_or("::")
            .to_string(),
        port: inbound
            .get("listen_port")
            .and_then(|v| v.as_u64())
            .unwrap_or_default() as u16,
        network: infer_network(inbound),
        link: None,
        client_json: None,
        created_at: iso_now(),
    })
}

fn infer_network(inbound: &JsonValue) -> String {
    match inbound
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
    {
        "hysteria2" | "tuic" => "udp".into(),
        "shadowsocks" => "tcp+udp".into(),
        _ => "tcp".into(),
    }
}

fn resolve_tls_material(
    matches: &ArgMatches,
    interactive: bool,
    server: &str,
) -> Result<(String, String, bool), CliError> {
    let cert = matches.get_one::<String>("cert-path").cloned();
    let key = matches.get_one::<String>("key-path").cloned();
    let self_signed = matches.get_flag("self-signed");
    if let (Some(cert), Some(key)) = (cert, key) {
        return Ok((cert, key, false));
    }
    if self_signed {
        return generate_self_signed(server);
    }
    if interactive {
        let use_self_signed = Confirm::new()
            .with_prompt("未提供证书路径，是否自动生成自签名证书？")
            .default(true)
            .interact()
            .map_err(|e| CliError::new(format!("读取确认失败: {}", e)))?;
        if use_self_signed {
            return generate_self_signed(server);
        }
        let cert_path = required_or_prompt(None, "请输入证书路径", true)?;
        let key_path = required_or_prompt(None, "请输入私钥路径", true)?;
        Ok((cert_path, key_path, false))
    } else {
        Err(CliError::new(
            "TLS 节点需要提供 --cert-path/--key-path，或显式传入 --self-signed",
        ))
    }
}

fn generate_self_signed(server: &str) -> Result<(String, String, bool), CliError> {
    let dir = PathBuf::from("/etc/sing-box/certs");
    ensure_dir(&dir)?;
    let cert = dir.join(format!("{}.crt", sanitize_name(server)));
    let key = dir.join(format!("{}.key", sanitize_name(server)));
    let status = SysCmd::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-sha256",
            "-nodes",
            "-keyout",
            key.to_string_lossy().as_ref(),
            "-out",
            cert.to_string_lossy().as_ref(),
            "-subj",
            &format!("/CN={}", server),
            "-days",
            "3650",
        ])
        .status()
        .map_err(|e| CliError::new(format!("生成自签名证书失败: {}", e)))?;
    if !status.success() {
        return Err(CliError::new("生成自签名证书失败"));
    }
    Ok((cert.display().to_string(), key.display().to_string(), true))
}

fn generate_reality_keypair() -> Result<(String, String), CliError> {
    if !Path::new(SINGBOX_BINARY_PATH).exists() {
        return Err(CliError::new(
            "自动生成 Reality 密钥前需要已安装 sing-box，可改为手动传入 --private-key 与 --public-key",
        ));
    }
    let output = SysCmd::new(SINGBOX_BINARY_PATH)
        .args(["generate", "reality-keypair"])
        .output()
        .map_err(|e| CliError::new(format!("生成 Reality 密钥失败: {}", e)))?;
    if !output.status.success() {
        return Err(CliError::new(format!(
            "生成 Reality 密钥失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut private_key = None;
    let mut public_key = None;
    for line in stdout.lines() {
        if line.contains("PrivateKey") {
            private_key = line.split(':').nth(1).map(|s| s.trim().to_string());
        }
        if line.contains("PublicKey") {
            public_key = line.split(':').nth(1).map(|s| s.trim().to_string());
        }
    }
    match (private_key, public_key) {
        (Some(privk), Some(pubk)) => Ok((privk, pubk)),
        _ => Err(CliError::new("无法解析自动生成的 Reality 密钥输出")),
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

fn resolve_server_name(
    explicit: Option<String>,
    server: &str,
    interactive: bool,
    prompt: &str,
    non_interactive_error: &str,
) -> Result<String, CliError> {
    if let Some(value) = explicit {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if !is_ip_host(server) {
        return Ok(server.to_string());
    }
    if interactive {
        return required_or_prompt(None, prompt, true);
    }
    Err(CliError::new(non_interactive_error))
}

fn parse_port(value: String) -> Result<u16, CliError> {
    let port: u16 = value.parse().map_err(|_| CliError::new("端口必须为数字"))?;
    if !(1..=65535).contains(&port) {
        return Err(CliError::new("端口范围必须是 1-65535"));
    }
    Ok(port)
}

fn is_port_in_use(port: u16, network: &str) -> bool {
    let flag = if network.contains("udp") && !network.contains("tcp") {
        "-lun"
    } else {
        "-ltn"
    };
    let output = SysCmd::new("sh")
        .arg("-c")
        .arg(format!("ss {} | grep -w ':{}'", flag, port))
        .output();
    output.map(|o| !o.stdout.is_empty()).unwrap_or(false)
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

fn new_uuid() -> String {
    random_uuid_v4().unwrap_or_else(|| {
        let output = SysCmd::new("uuidgen").output();
        output
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                format!(
                    "00000000-0000-4000-8000-{:012}",
                    current_timestamp() % 1_000_000_000_000
                )
            })
    })
}

fn random_uuid_v4() -> Option<String> {
    let mut bytes = [0u8; 16];
    let mut file = File::open("/dev/urandom").ok()?;
    file.read_exact(&mut bytes).ok()?;
    Some(format_uuid_v4(bytes))
}

fn format_uuid_v4(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
        u16::from_be_bytes([bytes[6], bytes[7]]),
        u16::from_be_bytes([bytes[8], bytes[9]]),
        ((bytes[10] as u64) << 40)
            | ((bytes[11] as u64) << 32)
            | ((bytes[12] as u64) << 24)
            | ((bytes[13] as u64) << 16)
            | ((bytes[14] as u64) << 8)
            | (bytes[15] as u64)
    )
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

fn strip_ip_brackets(host: &str) -> &str {
    host.trim().trim_start_matches('[').trim_end_matches(']')
}

fn is_ip_host(host: &str) -> bool {
    strip_ip_brackets(host).parse::<IpAddr>().is_ok()
}

fn uri_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{:02X}", *byte)),
        }
    }
    encoded
}

fn uri_fragment(value: &str) -> String {
    uri_component(value)
}

fn sanitize_name(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn iso_now() -> String {
    format!("{}", current_timestamp())
}

fn ss_methods() -> Vec<&'static str> {
    vec![
        "2022-blake3-aes-128-gcm",
        "2022-blake3-aes-256-gcm",
        "2022-blake3-chacha20-poly1305",
        "aes-128-gcm",
        "aes-256-gcm",
        "chacha20-ietf-poly1305",
        "xchacha20-ietf-poly1305",
    ]
}

fn validate_ss_method(method: &str) -> Result<String, CliError> {
    if ss_methods().contains(&method) {
        Ok(method.to_string())
    } else {
        Err(CliError::new(format!(
            "不支持的 Shadowsocks 方法：{}",
            method
        )))
    }
}

fn ss_password_for_method(method: &str) -> String {
    match method {
        "2022-blake3-aes-128-gcm" => {
            let output = SysCmd::new("openssl")
                .args(["rand", "-base64", "16"])
                .output();
            output
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_else(|| random_b64url(16))
        }
        "2022-blake3-aes-256-gcm" | "2022-blake3-chacha20-poly1305" => {
            let output = SysCmd::new("openssl")
                .args(["rand", "-base64", "32"])
                .output();
            output
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_else(|| random_b64url(32))
        }
        _ => random_b64url(24),
    }
    .trim()
    .to_string()
}

fn ss_userinfo(method: &str, password: &str) -> String {
    let payload = format!("{}:{}", method, password);
    let output = SysCmd::new("sh")
        .arg("-c")
        .arg(format!(
            "printf '%s' '{}' | base64 | tr '+/' '-_' | tr -d '=\\n'",
            payload.replace('\'', "'\\''")
        ))
        .output();
    output
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_version_accepts_plain_and_prefixed_values() {
        assert_eq!(normalize_version("1.13.12").unwrap(), "1.13.12");
        assert_eq!(normalize_version("v1.13.12").unwrap(), "1.13.12");
    }

    #[test]
    fn find_release_asset_uses_real_asset_name() {
        let release = GitHubRelease {
            tag_name: "v1.13.12".into(),
            assets: vec![GitHubAsset {
                name: "sing-box-1.13.12-linux-amd64.tar.gz".into(),
                browser_download_url: "https://example.invalid/sing-box-1.13.12-linux-amd64.tar.gz"
                    .into(),
            }],
        };
        let normalized = normalize_version(&release.tag_name).unwrap();
        let asset_name = format!("sing-box-{}-linux-{}.tar.gz", normalized, "amd64");
        let asset = release
            .assets
            .iter()
            .find(|item| item.name == asset_name)
            .unwrap();
        assert_eq!(asset.name, "sing-box-1.13.12-linux-amd64.tar.gz");
    }

    #[test]
    fn format_uuid_v4_sets_version_and_variant_bits() {
        let uuid = format_uuid_v4([0u8; 16]);
        assert_eq!(uuid, "00000000-0000-4000-8000-000000000000");
    }

    #[test]
    fn random_uuid_v4_has_expected_shape() {
        let uuid = random_uuid_v4().unwrap();
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.chars().nth(8), Some('-'));
        assert_eq!(uuid.chars().nth(13), Some('-'));
        assert_eq!(uuid.chars().nth(18), Some('-'));
        assert_eq!(uuid.chars().nth(23), Some('-'));
        assert_eq!(uuid.chars().nth(14), Some('4'));
        assert!(matches!(uuid.chars().nth(19), Some('8' | '9' | 'a' | 'b')));
    }

    #[test]
    fn resolve_server_name_reuses_domain_server() {
        let server_name =
            resolve_server_name(None, "edge.example.com", false, "ignored", "ignored").unwrap();
        assert_eq!(server_name, "edge.example.com");
    }

    #[test]
    fn resolve_server_name_requires_explicit_value_for_ip_in_non_interactive_mode() {
        let err = resolve_server_name(
            None,
            "1.2.3.4",
            false,
            "请输入 TLS SNI / 证书域名",
            "当 --server 为 IP 时，请显式提供 --server-name",
        )
        .unwrap_err();
        assert!(err.message.contains("显式提供 --server-name"));
    }

    #[test]
    fn uri_component_percent_encodes_reserved_characters() {
        assert_eq!(uri_component("a+b c/=?"), "a%2Bb%20c%2F%3D%3F");
    }
}
