use clap::{Arg, ArgMatches, Command, ArgAction};
use dialoguer::{Input, Confirm};
use std::path::PathBuf;

use crate::utils::{CliError, OutputFormat, OutputEnvelope, Breadcrumb};

use std::fs;
use std::io::{self, Read, Write};
use std::process::Command as SysCmd;
use std::path::{Path, PathBuf};
use flate2::read::GzDecoder;
use tar::Archive;
use sha2::{Sha256, Digest};
use hex;
use serde_json::{Value as JsonValue, json};
use std::fs::File;

pub fn cli() -> Command {
    Command::new("singbox")
        .about("管理 sing-box 安装和节点")
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
                .about("添加一个代理节点")
                .arg(
                    Arg::new("type")
                        .long("type")
                        .value_name("TYPE")
                        .help("节点类型，如 vless-reality、trojan-tls 等")
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
                    Arg::new("dry-run").long("dry-run").action(ArgAction::SetTrue)
                        .help("仅预览配置变更，不实际写入"),
                )
                .arg(
                    Arg::new("confirm").long("confirm").action(ArgAction::SetTrue)
                        .help("跳过交互确认，直接应用配置"),
                ),
        )
        .subcommand(
            Command::new("list-nodes")
                .about("列出当前所有节点")
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .value_name("N")
                        .help("返回的节点数量上限"),
                ),
        )
        .subcommand(
            Command::new("remove-node")
                .about("删除某个节点")
                .arg(
                    Arg::new("id")
                        .long("id")
                        .value_name("ID")
                        .help("要删除的节点 ID")
                        .required(true),
                )
                .arg(
                    Arg::new("confirm").long("confirm").action(ArgAction::SetTrue)
                        .help("跳过交互确认，直接删除"),
                ),
        )
        .subcommand(
            Command::new("status").about("查看 sing-box 服务状态"),
        )
        .subcommand(
            Command::new("start").about("启动 sing-box 服务"),
        )
        .subcommand(
            Command::new("stop").about("停止 sing-box 服务"),
        )
        .subcommand(
            Command::new("restart").about("重启 sing-box 服务"),
        )
}

pub fn handle_singbox(matches: &ArgMatches, format: OutputFormat, no_input: bool) -> Result<(), CliError> {
    match matches.subcommand() {
        Some(("install", sub)) => install_singbox(sub, format, no_input),
        Some(("add-node", sub)) => add_node(sub, format, no_input),
        Some(("list-nodes", sub)) => list_nodes(sub, format),
        Some(("remove-node", sub)) => remove_node(sub, format, no_input),
        Some(("status", _)) => service_action("status", format),
        Some(("start", _)) => service_action("start", format),
        Some(("stop", _)) => service_action("stop", format),
        Some(("restart", _)) => service_action("restart", format),
        _ => Err(CliError::new("未知子命令")),
    }
}

fn install_singbox(matches: &ArgMatches, format: OutputFormat, no_input: bool) -> Result<(), CliError> {
    let version = matches.get_one::<String>("version").cloned();
    let _sha256 = matches.get_one::<String>("sha256").cloned();
    let dry_run = matches.get_flag("dry-run");
    let confirm_flag = matches.get_flag("confirm");

    // interactive if TTY
    let interactive = atty::is(atty::Stream::Stdin) && !no_input;
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
    if interactive && !confirm_flag && !dry_run {
        let proceed = Confirm::new()
            .with_prompt("确认开始安装或更新 sing-box?")
            .default(true)
            .interact()
            .map_err(|e| CliError::new(format!("读取确认失败: {}", e)))?;
        if !proceed {
            return Ok(());
        }
    } else if !interactive && !confirm_flag && !dry_run {
        return Err(CliError::new("非交互模式安装必须使用 --confirm"));
    }

    if dry_run {
        match format {
            OutputFormat::Json => {
                let out: OutputEnvelope<serde_json::Value> = OutputEnvelope {
                    ok: true,
                    data: Some(serde_json::json!({"dry_run": true, "version": version })),
                    error: None,
                    breadcrumbs: Some(vec![Breadcrumb { action: "apply", cmd: "vps-cli singbox install --confirm".into() }]),
                };
                println!("{}", serde_json::to_string(&out).unwrap());
            }
            _ => {
                println!("[Dry Run] 将安装 sing-box 版本 {:?}", version);
            }
        }
        return Ok(());
    }

    // Perform installation steps
    // Determine version (default: latest). For simplicity, if version is None, we'll use "latest"
    let version_str = version.clone().unwrap_or_else(|| "latest".to_string());
    // Detect architecture
    let arch = detect_arch().ok_or_else(|| CliError::new("无法识别当前 CPU 架构"))?;
    // Build URL
    let download_url = if version_str == "latest" {
        // Use GitHub latest release tag by fetching from GitHub API is out-of-scope; fallback to predetermined url.
        format!("https://github.com/SagerNet/sing-box/releases/latest/download/sing-box-linux-{arch}.tar.gz")
    } else {
        format!("https://github.com/SagerNet/sing-box/releases/download/v{version}/sing-box-{version}-linux-{arch}.tar.gz", version = version_str, arch = arch)
    };
    // Prepare temporary directory
    let tmp_dir = PathBuf::from(format!("/tmp/singbox-install-{}", version_str));
    let _ = fs::create_dir_all(&tmp_dir);
    let tar_path = tmp_dir.join("sing-box.tar.gz");
    // Download file
    download_file(&download_url, &tar_path).map_err(|e| CliError::new(format!("下载失败: {}", e)))?;
    // Verify sha256 if provided
    if let Some(check) = _sha256 {
        let computed = compute_sha256(&tar_path).map_err(|e| CliError::new(format!("计算 sha256 失败: {}", e)))?;
        if computed.to_lowercase() != check.to_lowercase() {
            return Err(CliError::new(format!("SHA256 校验失败: 期待 {}, 计算得 {}", check, computed)));
        }
    }
    // Extract
    extract_tar(&tar_path, &tmp_dir).map_err(|e| CliError::new(format!("解压失败: {}", e)))?;
    // The extracted directory is sing-box-{version}-linux-{arch}
    let extracted_subdir = if version_str == "latest" {
        // Unknown folder name; search for directory starting with "sing-box" in tmp_dir
        let entries: Vec<_> = fs::read_dir(&tmp_dir).unwrap().filter_map(|e| e.ok()).collect();
        let mut target: Option<PathBuf> = None;
        for e in entries {
            let name = e.file_name().into_string().unwrap_or_default();
            if name.starts_with("sing-box") {
                target = Some(e.path());
                break;
            }
        }
        target.ok_or_else(|| CliError::new("未找到解压后的 sing-box 目录"))?
    } else {
        tmp_dir.join(format!("sing-box-{version}-linux-{arch}", version = version_str, arch = arch))
    };
    // Install binary
    install_binary(&extracted_subdir).map_err(|e| CliError::new(format!("安装二进制失败: {}", e)))?;
    // Create system user and directories
    create_system_user().map_err(|e| CliError::new(format!("创建系统用户失败: {}", e)))?;
    create_dirs().map_err(|e| CliError::new(format!("创建目录失败: {}", e)))?;
    // Create systemd service
    create_service_file().map_err(|e| CliError::new(format!("创建 systemd 服务失败: {}", e)))?;
    // Reload and enable service
    reload_systemd().map_err(|e| CliError::new(format!("重载 systemd 失败: {}", e)))?;
    enable_service().map_err(|e| CliError::new(format!("启用服务失败: {}", e)))?;

    // Create default config if not exists
    if read_singbox_config().is_none() {
        let default_cfg = json!({
            "log": {"disabled": false},
            "outbounds": [] as Vec<JsonValue>
        });
        // ignore error, just attempt
        let _ = write_singbox_config(&default_cfg);
    }

    match format {
        OutputFormat::Json => {
            let out: OutputEnvelope<serde_json::Value> = OutputEnvelope {
                ok: true,
                data: Some(serde_json::json!({"installed": true, "version": version_str })),
                error: None,
                breadcrumbs: Some(vec![Breadcrumb { action: "add-node", cmd: "vps-cli singbox add-node --type <type> --config-file <file> --json".into() }]),
            };
            println!("{}", serde_json::to_string(&out).unwrap());
        }
        _ => {
            println!("已成功安装/更新 sing-box。");
            println!("版本: {}", version_str);
        }
    }

    Ok(())
}

fn add_node(matches: &ArgMatches, format: OutputFormat, no_input: bool) -> Result<(), CliError> {
    let node_type = matches.get_one::<String>("type").unwrap().clone();
    let config_file = matches.get_one::<String>("config-file").unwrap().clone();
    let dry_run = matches.get_flag("dry-run");
    let confirm_flag = matches.get_flag("confirm");

    let interactive = atty::is(atty::Stream::Stdin) && !no_input;
    if interactive && !confirm_flag && !dry_run {
        let proceed = Confirm::new()
            .with_prompt(format!("确认添加 {} 节点?", node_type))
            .default(true)
            .interact()
            .map_err(|e| CliError::new(format!("读取确认失败: {}", e)))?;
        if !proceed { return Ok(()); }
    } else if !interactive && !confirm_flag && !dry_run {
        return Err(CliError::new("非交互模式添加节点必须使用 --confirm"));
    }

    // Validate config file path
    let path = PathBuf::from(&config_file);
    if !path.exists() {
        return Err(CliError::new(format!("配置文件不存在: {}", config_file)));
    }

    if dry_run {
        match format {
            OutputFormat::Json => {
                let out: OutputEnvelope<JsonValue> = OutputEnvelope {
                    ok: true,
                    data: Some(json!({"dry_run": true, "type": node_type, "config_file": config_file })),
                    error: None,
                    breadcrumbs: Some(vec![Breadcrumb { action: "apply".into(), cmd: "vps-cli singbox add-node --confirm ...".into() }]),
                };
                println!("{}", serde_json::to_string(&out).unwrap());
            }
            _ => {
                println!("[Dry Run] 将添加节点类型 {}，配置文件 {}", node_type, config_file);
            }
        }
        return Ok(());
    }

    // Read existing config or create default
    let mut config = read_singbox_config().unwrap_or_else(|| json!({"outbounds": []}));
    // Ensure outbounds is array
    if !config.get("outbounds").map(|v| v.is_array()).unwrap_or(false) {
        config["outbounds"] = JsonValue::Array(vec![]);
    }
    let outbounds = config.get_mut("outbounds").unwrap().as_array_mut().unwrap();
    // Determine new id
    let new_id = format!("node{}", outbounds.len() + 1);
    // Parse node configuration from file
    let node_value: JsonValue = {
        let file = fs::File::open(&path).map_err(|e| CliError::new(format!("读取配置文件失败: {}", e)))?;
        serde_json::from_reader(file).map_err(|e| CliError::new(format!("解析配置文件失败: {}", e)))?
    };
    // If the node config is an array, merge all elements; if it's an object, treat as single
    if let JsonValue::Array(arr) = node_value {
        for mut n in arr {
            if let JsonValue::Object(ref mut map) = n {
                if !map.contains_key("tag") {
                    map.insert("tag".into(), JsonValue::String(new_id.clone()));
                }
            }
            outbounds.push(n);
        }
    } else if let JsonValue::Object(mut map) = node_value {
        if !map.contains_key("tag") {
            map.insert("tag".into(), JsonValue::String(new_id.clone()));
        }
        outbounds.push(JsonValue::Object(map));
    } else {
        return Err(CliError::new("节点配置文件格式不正确，应为 JSON 对象或数组"));
    }
    // Write config back
    write_singbox_config(&config).map_err(|e| CliError::new(format!("写入配置失败: {}", e)))?;

    match format {
        OutputFormat::Json => {
            let out: OutputEnvelope<JsonValue> = OutputEnvelope {
                ok: true,
                data: Some(json!({"type": node_type, "id": new_id})),
                error: None,
                breadcrumbs: Some(vec![Breadcrumb { action: "list".into(), cmd: "vps-cli singbox list-nodes --json".into() }]),
            };
            println!("{}", serde_json::to_string(&out).unwrap());
        }
        _ => {
            println!("成功添加节点 {}，ID: {}", node_type, new_id);
        }
    }
    Ok(())
}

fn list_nodes(matches: &ArgMatches, format: OutputFormat) -> Result<(), CliError> {
    let limit = matches.get_one::<String>("limit").and_then(|v| v.parse::<usize>().ok());
    // Read config
    let config = read_singbox_config().unwrap_or_else(|| json!({"outbounds": []}));
    let mut nodes: Vec<(String, String)> = Vec::new();
    if let Some(outbounds) = config.get("outbounds").and_then(|v| v.as_array()) {
        for (idx, ob) in outbounds.iter().enumerate() {
            let id = ob.get("tag").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("{}", idx + 1));
            // Determine node type from "type" or "protocol"
            let typ = ob.get("type")
                .or_else(|| ob.get("protocol"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            nodes.push((id, typ));
        }
    }
    // Limit
    let total = nodes.len();
    if let Some(l) = limit {
        nodes.truncate(l);
    }
    match format {
        OutputFormat::Json => {
            let data: Vec<_> = nodes.iter().map(|(id, typ)| json!({"id": id, "type": typ})).collect();
            let mut breadcrumbs = None;
            if limit.is_none() && total > nodes.len() {
                breadcrumbs = Some(vec![Breadcrumb { action: "limit".into(), cmd: "vps-cli singbox list-nodes --limit 10 --json".into() }]);
            }
            let out: OutputEnvelope<Vec<JsonValue>> = OutputEnvelope {
                ok: true,
                data: Some(data),
                error: None,
                breadcrumbs,
            };
            println!("{}", serde_json::to_string(&out).unwrap());
        }
        OutputFormat::Plain => {
            for (id, typ) in &nodes {
                println!("{}\t{}", id, typ);
            }
            if limit.is_none() && total > nodes.len() {
                eprintln!("显示 {} 条记录，使用 --limit 缩小输出", nodes.len());
            }
        }
        OutputFormat::Human => {
            println!("节点列表:");
            for (id, typ) in &nodes {
                println!("- ID: {}, 类型: {}", id, typ);
            }
            if limit.is_none() && total > nodes.len() {
                println!("\n共 {} 条记录，建议使用 --limit 缩小输出。", nodes.len());
            }
        }
    }
    Ok(())
}

fn remove_node(matches: &ArgMatches, format: OutputFormat, no_input: bool) -> Result<(), CliError> {
    let id = matches.get_one::<String>("id").unwrap().clone();
    let confirm_flag = matches.get_flag("confirm");
    let interactive = atty::is(atty::Stream::Stdin) && !no_input;
    if interactive && !confirm_flag {
        let proceed = Confirm::new()
            .with_prompt(format!("确认删除节点 {}?", id))
            .default(false)
            .interact()
            .map_err(|e| CliError::new(format!("读取确认失败: {}", e)))?;
        if !proceed { return Ok(()); }
    } else if !interactive && !confirm_flag {
        return Err(CliError::new("非交互模式删除节点必须使用 --confirm"));
    }
    // Read config
    let mut config = read_singbox_config().unwrap_or_else(|| json!({"outbounds": []}));
    let mut removed = false;
    if let Some(outbounds) = config.get_mut("outbounds").and_then(|v| v.as_array_mut()) {
        // Determine removal by tag or index
        if id.chars().all(|c| c.is_ascii_digit()) {
            // numeric index (1-based)
            if let Ok(idx) = id.parse::<usize>() {
                if idx > 0 && idx <= outbounds.len() {
                    outbounds.remove(idx - 1);
                    removed = true;
                }
            }
        } else {
            // tag
            if let Some(pos) = outbounds.iter().position(|n| n.get("tag").and_then(|v| v.as_str()) == Some(id.as_str())) {
                outbounds.remove(pos);
                removed = true;
            }
        }
    }
    if !removed {
        return Err(CliError::new(format!("未找到节点 {}", id)));
    }
    // Write back
    write_singbox_config(&config).map_err(|e| CliError::new(format!("写入配置失败: {}", e)))?;
    match format {
        OutputFormat::Json => {
            let out: OutputEnvelope<JsonValue> = OutputEnvelope {
                ok: true,
                data: Some(json!({"removed": id.clone()})),
                error: None,
                breadcrumbs: Some(vec![Breadcrumb { action: "list".into(), cmd: "vps-cli singbox list-nodes --json".into() }]),
            };
            println!("{}", serde_json::to_string(&out).unwrap());
        }
        _ => {
            println!("已删除节点 {}", id);
        }
    }
    Ok(())
}

fn service_action(action: &str, format: OutputFormat) -> Result<(), CliError> {
    // Interact with systemctl
    let service_name = "sing-box";
    let result = match action {
        "status" => SysCmd::new("systemctl").args(&["is-active", service_name]).output(),
        "start" => SysCmd::new("systemctl").args(&["start", service_name]).output(),
        "stop" => SysCmd::new("systemctl").args(&["stop", service_name]).output(),
        "restart" => SysCmd::new("systemctl").args(&["restart", service_name]).output(),
        _ => return Err(CliError::new("未知操作")),
    };
    match result {
        Ok(output) => {
            let success = output.status.success();
            match action {
                "status" => {
                    let status_str = if success { String::from_utf8_lossy(&output.stdout).trim().to_string() } else { "unknown".to_string() };
                    match format {
                        OutputFormat::Json => {
                            let out: OutputEnvelope<JsonValue> = OutputEnvelope {
                                ok: true,
                                data: Some(json!({"status": status_str})),
                                error: None,
                                breadcrumbs: Some(vec![Breadcrumb { action: "restart".into(), cmd: "vps-cli singbox restart".into() }]),
                            };
                            println!("{}", serde_json::to_string(&out).unwrap());
                        }
                        _ => println!("sing-box 服务状态: {}", status_str),
                    }
                }
                "start" => {
                    match format {
                        OutputFormat::Json => {
                            let out: OutputEnvelope<JsonValue> = OutputEnvelope {
                                ok: success,
                                data: Some(json!({"started": success})),
                                error: None,
                                breadcrumbs: Some(vec![Breadcrumb { action: "status".into(), cmd: "vps-cli singbox status".into() }]),
                            };
                            println!("{}", serde_json::to_string(&out).unwrap());
                        }
                        _ => {
                            if success { println!("已启动 sing-box 服务"); } else { println!("启动 sing-box 服务失败"); }
                        }
                    }
                }
                "stop" => {
                    match format {
                        OutputFormat::Json => {
                            let out: OutputEnvelope<JsonValue> = OutputEnvelope {
                                ok: success,
                                data: Some(json!({"stopped": success})),
                                error: None,
                                breadcrumbs: Some(vec![Breadcrumb { action: "start".into(), cmd: "vps-cli singbox start".into() }]),
                            };
                            println!("{}", serde_json::to_string(&out).unwrap());
                        }
                        _ => {
                            if success { println!("已停止 sing-box 服务"); } else { println!("停止 sing-box 服务失败"); }
                        }
                    }
                }
                "restart" => {
                    match format {
                        OutputFormat::Json => {
                            let out: OutputEnvelope<JsonValue> = OutputEnvelope {
                                ok: success,
                                data: Some(json!({"restarted": success})),
                                error: None,
                                breadcrumbs: Some(vec![Breadcrumb { action: "status".into(), cmd: "vps-cli singbox status".into() }]),
                            };
                            println!("{}", serde_json::to_string(&out).unwrap());
                        }
                        _ => {
                            if success { println!("已重启 sing-box 服务"); } else { println!("重启 sing-box 服务失败"); }
                        }
                    }
                }
                _ => {}
            }
        }
        Err(e) => return Err(CliError::new(format!("执行 systemctl 命令失败: {}", e))),
    }
    Ok(())
}

/// Detects system architecture and returns sing-box release suffix.
fn detect_arch() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" | "amd64" => Some("amd64"),
        "aarch64" => Some("arm64"),
        "armv7" => Some("armv7"),
        "i386" | "i686" => Some("386"),
        _ => None,
    }
}

/// Path to sing-box configuration file
const SINGBOX_CONFIG_PATH: &str = "/etc/sing-box/config.json";

/// Read sing-box config JSON. Returns None if file does not exist or cannot be parsed.
fn read_singbox_config() -> Option<JsonValue> {
    let path = Path::new(SINGBOX_CONFIG_PATH);
    if !path.exists() {
        return None;
    }
    let file = File::open(path).ok()?;
    let json: Result<JsonValue, _> = serde_json::from_reader(file);
    json.ok()
}

/// Write sing-box config JSON
fn write_singbox_config(config: &JsonValue) -> Result<(), io::Error> {
    let path = Path::new(SINGBOX_CONFIG_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    let data = serde_json::to_vec_pretty(config).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    file.write_all(&data)?;
    Ok(())
}

fn download_file(url: &str, dest: &Path) -> Result<(), io::Error> {
    let response = reqwest::blocking::get(url).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    if !response.status().is_success() {
        return Err(io::Error::new(io::ErrorKind::Other, format!("下载失败: HTTP {}", response.status())));
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
        if n == 0 { break; }
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
        return Err(io::Error::new(io::ErrorKind::NotFound, "未找到 sing-box 可执行文件"));
    }
    // Copy to /usr/local/bin
    fs::create_dir_all("/usr/local/bin")?;
    fs::copy(&bin_path, "/usr/local/bin/sing-box")?;
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata("/usr/local/bin/sing-box")?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions("/usr/local/bin/sing-box", perms)?;
    Ok(())
}

fn create_system_user() -> Result<(), io::Error> {
    // Check if user exists
    let status = SysCmd::new("id").arg("-u").arg("sing-box").status();
    if let Ok(st) = status {
        if st.success() {
            return Ok(());
        }
    }
    // Create user
    let status = SysCmd::new("useradd")
        .args(&["--system", "--no-create-home", "--shell", "/usr/sbin/nologin", "sing-box"])
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
    fs::write("/etc/systemd/system/sing-box.service", service_content)?;
    Ok(())
}

fn reload_systemd() -> Result<(), io::Error> {
    let status = SysCmd::new("systemctl").arg("daemon-reload").status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "systemctl daemon-reload 失败"))
    }
}

fn enable_service() -> Result<(), io::Error> {
    let status = SysCmd::new("systemctl").args(&["enable", "--now", "sing-box"]).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "启动 sing-box 服务失败"))
    }
}
