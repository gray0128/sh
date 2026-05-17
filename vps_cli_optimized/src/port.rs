use clap::{Arg, ArgAction, ArgMatches, Command};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::process::Command as SysCmd;

use crate::utils::{Breadcrumb, CliError, OperationReport, OutputEnvelope, OutputFormat};

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct PortUsageEntry {
    protocol: String,
    state: String,
    local_address: String,
    port: u16,
    pid: Option<u32>,
    process: Option<String>,
}

#[derive(Debug, Serialize)]
struct PortUsageOutput {
    backend: String,
    filter_port: Option<u16>,
    listening_only: bool,
    entry_count: usize,
    entries: Vec<PortUsageEntry>,
}

pub fn cli() -> Command {
    Command::new("port")
        .about("查看端口占用与监听进程")
        .subcommand(
            Command::new("usage")
                .about("查看端口占用情况")
                .arg(
                    Arg::new("port")
                        .long("port")
                        .value_name("PORT")
                        .help("仅查看指定端口"),
                )
                .arg(
                    Arg::new("all")
                        .long("all")
                        .action(ArgAction::SetTrue)
                        .help("包含非监听连接"),
                ),
        )
}

pub fn handle_port(
    matches: &ArgMatches,
    format: OutputFormat,
    _no_input: bool,
) -> Result<(), CliError> {
    match matches.subcommand() {
        Some(("usage", sub)) => handle_usage(sub, format),
        _ => Err(CliError::new("未知子命令")),
    }
}

fn handle_usage(matches: &ArgMatches, format: OutputFormat) -> Result<(), CliError> {
    let filter_port = matches
        .get_one::<String>("port")
        .cloned()
        .map(parse_port)
        .transpose()?;
    let listening_only = !matches.get_flag("all");

    let mut report = OperationReport::default();
    let (backend, entries) = query_port_usage(filter_port, listening_only, &mut report.warnings)?;
    let data = PortUsageOutput {
        backend: backend.into(),
        filter_port,
        listening_only,
        entry_count: entries.len(),
        entries,
    };

    emit_success(
        format,
        &data,
        &report,
        Some(vec![crumb("query_port_usage", "vps-cli port usage")]),
    );
    Ok(())
}

fn query_port_usage(
    filter_port: Option<u16>,
    listening_only: bool,
    warnings: &mut Vec<String>,
) -> Result<(&'static str, Vec<PortUsageEntry>), CliError> {
    if let Some(entries) = query_ss(filter_port, listening_only, warnings)? {
        return Ok(("ss", entries));
    }
    if let Some(entries) = query_lsof(filter_port, listening_only, warnings)? {
        return Ok(("lsof", entries));
    }
    Err(CliError::new(
        "未找到可用的端口查询工具，请安装 ss 或 lsof 后重试",
    ))
}

fn query_ss(
    filter_port: Option<u16>,
    listening_only: bool,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<PortUsageEntry>>, CliError> {
    if !command_exists("ss") {
        return Ok(None);
    }

    let mut args = vec!["-H", "-n", "-p"];
    if listening_only {
        args.push("-l");
    } else {
        args.push("-a");
    }
    args.push("-t");
    args.push("-u");

    let output = SysCmd::new("ss")
        .args(&args)
        .output()
        .map_err(|e| CliError::new(format!("执行 ss 查询失败: {}", e)))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !detail.is_empty() {
            warnings.push(format!("ss 查询失败，回退到 lsof: {}", detail));
        }
        return Ok(None);
    }

    let entries = filter_entries(
        parse_ss_output(&String::from_utf8_lossy(&output.stdout)),
        filter_port,
    );
    Ok(Some(entries))
}

fn query_lsof(
    filter_port: Option<u16>,
    listening_only: bool,
    _warnings: &mut Vec<String>,
) -> Result<Option<Vec<PortUsageEntry>>, CliError> {
    if !command_exists("lsof") {
        return Ok(None);
    }

    let mut args = vec!["-nP".to_string()];
    if let Some(port) = filter_port {
        args.push("-i".to_string());
        args.push(format!(":{}", port));
    } else {
        args.push("-iTCP".to_string());
        args.push("-iUDP".to_string());
    }
    if listening_only {
        args.push("-sTCP:LISTEN".to_string());
    }

    let output = SysCmd::new("lsof")
        .args(&args)
        .output()
        .map_err(|e| CliError::new(format!("执行 lsof 查询失败: {}", e)))?;

    // lsof 在无匹配时通常以非 0 退出码返回，此时视为成功但结果为空。
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && stdout.trim().is_empty() && !stderr.contains("No such file") {
        return Err(CliError::new(format!(
            "执行 lsof 查询失败: {}",
            stderr.trim()
        )));
    }

    let entries = filter_entries(parse_lsof_output(&stdout), filter_port);
    Ok(Some(entries))
}

fn command_exists(program: &str) -> bool {
    SysCmd::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", program))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn parse_port(value: String) -> Result<u16, CliError> {
    let port: u16 = value.parse().map_err(|_| CliError::new("端口必须为数字"))?;
    if port == 0 {
        return Err(CliError::new("端口范围必须为 1-65535"));
    }
    Ok(port)
}

fn parse_ss_output(stdout: &str) -> Vec<PortUsageEntry> {
    let mut entries = Vec::new();
    for line in stdout.lines() {
        let mut fields = line.split_whitespace();
        let Some(protocol) = fields.next() else {
            continue;
        };
        let Some(state) = fields.next() else {
            continue;
        };
        let _recv_q = fields.next();
        let _send_q = fields.next();
        let Some(local_address) = fields.next() else {
            continue;
        };
        let _peer_address = fields.next();
        let process_info = fields.collect::<Vec<_>>().join(" ");
        let Some(port) = extract_port(local_address) else {
            continue;
        };
        let (process, pid) = parse_ss_process(&process_info);
        entries.push(PortUsageEntry {
            protocol: protocol.to_string(),
            state: state.to_string(),
            local_address: local_address.to_string(),
            port,
            pid,
            process,
        });
    }
    entries
}

fn parse_ss_process(value: &str) -> (Option<String>, Option<u32>) {
    let process = value.find('"').and_then(|start| {
        let rest = &value[start + 1..];
        rest.find('"').map(|end| rest[..end].to_string())
    });
    let pid = value.find("pid=").and_then(|idx| {
        value[idx + 4..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>()
            .parse::<u32>()
            .ok()
    });
    (process, pid)
}

fn parse_lsof_output(stdout: &str) -> Vec<PortUsageEntry> {
    let mut entries = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() || line.starts_with("COMMAND") {
            continue;
        }
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 9 {
            continue;
        }
        let Some(pid) = columns[1].parse::<u32>().ok() else {
            continue;
        };
        let protocol = columns[7].to_lowercase();
        let descriptor = columns[8..].join(" ");
        let name_fields: Vec<&str> = descriptor.split_whitespace().collect();
        if name_fields.is_empty() {
            continue;
        }
        let local_address = name_fields[0]
            .split("->")
            .next()
            .unwrap_or(name_fields[0])
            .to_string();
        let Some(port) = extract_port(&local_address) else {
            continue;
        };
        let state = descriptor
            .split('(')
            .nth(1)
            .and_then(|segment| segment.split(')').next())
            .unwrap_or_else(|| if protocol == "udp" { "UNCONN" } else { "UNKNOWN" })
            .to_string();
        entries.push(PortUsageEntry {
            protocol,
            state,
            local_address,
            port,
            pid: Some(pid),
            process: Some(columns[0].to_string()),
        });
    }
    entries
}

fn extract_port(local_address: &str) -> Option<u16> {
    let port = local_address.rsplit(':').next()?;
    port.parse::<u16>().ok()
}

fn filter_entries(entries: Vec<PortUsageEntry>, filter_port: Option<u16>) -> Vec<PortUsageEntry> {
    let mut filtered = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in entries {
        if filter_port.is_some() && Some(entry.port) != filter_port {
            continue;
        }
        if seen.insert(entry.clone()) {
            filtered.push(entry);
        }
    }
    filtered
}

fn emit_success(
    format: OutputFormat,
    data: &PortUsageOutput,
    report: &OperationReport,
    breadcrumbs: Option<Vec<Breadcrumb>>,
) {
    match format {
        OutputFormat::Json => {
            let env = if let Some(crumbs) = breadcrumbs {
                OutputEnvelope::success(json!(data))
                    .with_report(report)
                    .with_breadcrumbs(crumbs)
            } else {
                OutputEnvelope::success(json!(data)).with_report(report)
            };
            println!("{}", serde_json::to_string(&env).unwrap());
        }
        OutputFormat::Plain => {
            println!("{}", render_plain_tsv(data));
            if !report.warnings.is_empty() {
                eprintln!();
                for item in &report.warnings {
                    eprintln!("警告: {}", item);
                }
            }
        }
        OutputFormat::Human => {
            println!("{}", render_human_table(data));
            if !report.warnings.is_empty() {
                println!("\n警告:");
                for item in &report.warnings {
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

fn render_plain_tsv(data: &PortUsageOutput) -> String {
    let mut lines =
        vec!["protocol\tstate\tlocal_address\tport\tpid\tprocess\tbackend".to_string()];
    for entry in &data.entries {
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            entry.protocol,
            entry.state,
            entry.local_address,
            entry.port,
            entry
                .pid
                .map(|value| value.to_string())
                .unwrap_or_default(),
            entry.process.clone().unwrap_or_default(),
            data.backend
        ));
    }
    lines.join("\n")
}

fn render_human_table(data: &PortUsageOutput) -> String {
    let mut lines = vec![
        format!("查询工具: {}", data.backend),
        format!(
            "筛选端口: {}",
            data.filter_port
                .map(|port| port.to_string())
                .unwrap_or_else(|| "全部".to_string())
        ),
        format!(
            "连接范围: {}",
            if data.listening_only {
                "仅监听"
            } else {
                "全部连接"
            }
        ),
        format!("匹配记录: {}", data.entry_count),
    ];
    lines.push(render_entries_table(&data.entries));
    lines.join("\n")
}

fn render_entries_table(entries: &[PortUsageEntry]) -> String {
    let protocol_width = entries
        .iter()
        .map(|item| item.protocol.len())
        .max()
        .unwrap_or(2)
        .max("协议".len());
    let state_width = entries
        .iter()
        .map(|item| item.state.len())
        .max()
        .unwrap_or(2)
        .max("状态".len());
    let address_width = entries
        .iter()
        .map(|item| item.local_address.len())
        .max()
        .unwrap_or(4)
        .max("本地地址".len());
    let port_width = entries
        .iter()
        .map(|item| item.port.to_string().len())
        .max()
        .unwrap_or(2)
        .max("端口".len());
    let pid_width = entries
        .iter()
        .map(|item| item.pid.map(|value| value.to_string().len()).unwrap_or(1))
        .max()
        .unwrap_or(2)
        .max("PID".len());
    let process_width = entries
        .iter()
        .map(|item| item.process.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(2)
        .max("进程".len());

    let header = format!(
        "{:<protocol_width$}  {:<state_width$}  {:<address_width$}  {:<port_width$}  {:<pid_width$}  {:<process_width$}",
        "协议", "状态", "本地地址", "端口", "PID", "进程"
    );
    let separator = format!(
        "{:-<protocol_width$}  {:-<state_width$}  {:-<address_width$}  {:-<port_width$}  {:-<pid_width$}  {:-<process_width$}",
        "", "", "", "", "", ""
    );
    let mut lines = vec![header, separator];
    if entries.is_empty() {
        lines.push("无匹配记录".to_string());
        return lines.join("\n");
    }

    for entry in entries {
        lines.push(format!(
            "{:<protocol_width$}  {:<state_width$}  {:<address_width$}  {:<port_width$}  {:<pid_width$}  {:<process_width$}",
            entry.protocol,
            entry.state,
            entry.local_address,
            entry.port,
            entry
                .pid
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            entry.process.as_deref().unwrap_or("-"),
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ss_output_extracts_process_and_port() {
        let stdout = r#"tcp LISTEN 0 4096 127.0.0.1:8080 0.0.0.0:* users:(("python3",pid=9527,fd=3))
udp UNCONN 0 0 0.0.0.0:5353 0.0.0.0:* users:(("mdns",pid=7,fd=5))"#;

        let entries = parse_ss_output(stdout);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].protocol, "tcp");
        assert_eq!(entries[0].port, 8080);
        assert_eq!(entries[0].pid, Some(9527));
        assert_eq!(entries[0].process.as_deref(), Some("python3"));
        assert_eq!(entries[1].protocol, "udp");
        assert_eq!(entries[1].port, 5353);
    }

    #[test]
    fn parse_lsof_output_extracts_listener_rows() {
        let stdout = r#"COMMAND   PID USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME
Python   9527 user    3u  IPv4 0x123456789abcdef0      0t0  TCP 127.0.0.1:8080 (LISTEN)
dnsmasq   777 root    5u  IPv4 0xfedcba9876543210      0t0  UDP *:53"#;

        let entries = parse_lsof_output(stdout);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].protocol, "tcp");
        assert_eq!(entries[0].state, "LISTEN");
        assert_eq!(entries[0].port, 8080);
        assert_eq!(entries[0].process.as_deref(), Some("Python"));
        assert_eq!(entries[1].protocol, "udp");
        assert_eq!(entries[1].state, "UNCONN");
        assert_eq!(entries[1].port, 53);
    }

    #[test]
    fn render_plain_tsv_keeps_header_when_empty() {
        let data = PortUsageOutput {
            backend: "ss".into(),
            filter_port: None,
            listening_only: true,
            entry_count: 0,
            entries: vec![],
        };

        assert_eq!(
            render_plain_tsv(&data),
            "protocol\tstate\tlocal_address\tport\tpid\tprocess\tbackend"
        );
    }
}
