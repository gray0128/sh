use clap::{Arg, ArgMatches, Command};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::process::Command as SysCmd;

use crate::utils::{Breadcrumb, CliError, OperationReport, OutputEnvelope, OutputFormat};

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct FirewallPortEntry {
    port: String,
    protocol: String,
    family: String,
    action: String,
}

#[derive(Debug, Serialize)]
struct FirewallPortsOutput {
    backend: String,
    filter_port: Option<u16>,
    entry_count: usize,
    entries: Vec<FirewallPortEntry>,
}

pub fn cli() -> Command {
    Command::new("firewall")
        .about("查看防火墙开放端口与协议（仅限 Ubuntu）")
        .subcommand(
            Command::new("ports").about("查看开放端口与协议").arg(
                Arg::new("port")
                    .long("port")
                    .value_name("PORT")
                    .help("仅查看指定端口的规则"),
            ),
        )
}

pub fn handle_firewall(
    matches: &ArgMatches,
    format: OutputFormat,
    _no_input: bool,
) -> Result<(), CliError> {
    match matches.subcommand() {
        Some(("ports", sub)) => handle_ports(sub, format),
        _ => Err(CliError::new("未知子命令")),
    }
}

fn handle_ports(matches: &ArgMatches, format: OutputFormat) -> Result<(), CliError> {
    ensure_root_and_ubuntu()?;
    let filter_port = matches
        .get_one::<String>("port")
        .cloned()
        .map(parse_port)
        .transpose()?;

    let mut report = OperationReport::default();
    let (backend, entries) = query_firewall_ports(filter_port, &mut report.warnings)?;
    let data = FirewallPortsOutput {
        backend: backend.into(),
        filter_port,
        entry_count: entries.len(),
        entries,
    };

    emit_success(
        format,
        &data,
        &report,
        Some(vec![crumb(
            "query_firewall_ports",
            "vps-cli firewall ports",
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

fn query_firewall_ports(
    filter_port: Option<u16>,
    warnings: &mut Vec<String>,
) -> Result<(&'static str, Vec<FirewallPortEntry>), CliError> {
    if let Some(entries) = query_ufw(filter_port, warnings)? {
        return Ok(("ufw", entries));
    }
    if let Some(entries) = query_nftables(filter_port, warnings)? {
        return Ok(("nftables", entries));
    }
    if let Some(entries) = query_iptables(filter_port, warnings)? {
        return Ok(("iptables", entries));
    }
    Err(CliError::new(
        "未检测到可查询的防火墙规则，请确认系统已安装并启用 ufw、nftables 或 iptables",
    ))
}

fn query_ufw(
    filter_port: Option<u16>,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<FirewallPortEntry>>, CliError> {
    if !command_exists("ufw") {
        return Ok(None);
    }
    let output = SysCmd::new("ufw")
        .arg("status")
        .output()
        .map_err(|e| CliError::new(format!("执行 ufw status 失败: {}", e)))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !detail.is_empty() {
            warnings.push(format!(
                "ufw status 执行失败，继续检查其它防火墙: {}",
                detail
            ));
        }
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("Status: inactive") {
        warnings.push("检测到 ufw 已安装但未启用，继续检查 nftables/iptables".into());
        return Ok(None);
    }
    if stdout.contains("Status: active") {
        let entries = filter_entries(parse_ufw_status(&stdout), filter_port);
        return Ok(Some(entries));
    }

    warnings.push("无法识别 ufw 状态输出，继续检查 nftables/iptables".into());
    Ok(None)
}

fn query_nftables(
    filter_port: Option<u16>,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<FirewallPortEntry>>, CliError> {
    if !command_exists("nft") {
        return Ok(None);
    }
    let output = SysCmd::new("nft")
        .args(["list", "ruleset"])
        .output()
        .map_err(|e| CliError::new(format!("执行 nft list ruleset 失败: {}", e)))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !detail.is_empty() {
            warnings.push(format!(
                "nft list ruleset 执行失败，继续检查 iptables: {}",
                detail
            ));
        }
        return Ok(None);
    }

    let all_entries = parse_nft_ruleset(&String::from_utf8_lossy(&output.stdout));
    if all_entries.is_empty() {
        warnings.push("未在 nftables 中识别到开放端口规则，继续检查 iptables".into());
        return Ok(None);
    }
    Ok(Some(filter_entries(all_entries, filter_port)))
}

fn query_iptables(
    filter_port: Option<u16>,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<FirewallPortEntry>>, CliError> {
    let mut all_entries = Vec::new();
    let mut available = false;

    if command_exists("iptables") {
        available = true;
        let output = SysCmd::new("iptables")
            .args(["-S", "INPUT"])
            .output()
            .map_err(|e| CliError::new(format!("执行 iptables -S INPUT 失败: {}", e)))?;
        if output.status.success() {
            all_entries.extend(parse_iptables_rules(
                &String::from_utf8_lossy(&output.stdout),
                "ipv4",
            ));
        } else {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !detail.is_empty() {
                warnings.push(format!("iptables 查询失败: {}", detail));
            }
        }
    }

    if command_exists("ip6tables") {
        available = true;
        let output = SysCmd::new("ip6tables")
            .args(["-S", "INPUT"])
            .output()
            .map_err(|e| CliError::new(format!("执行 ip6tables -S INPUT 失败: {}", e)))?;
        if output.status.success() {
            all_entries.extend(parse_iptables_rules(
                &String::from_utf8_lossy(&output.stdout),
                "ipv6",
            ));
        } else {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !detail.is_empty() {
                warnings.push(format!("ip6tables 查询失败: {}", detail));
            }
        }
    }

    if !available {
        return Ok(None);
    }
    Ok(Some(filter_entries(all_entries, filter_port)))
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

fn filter_entries(
    entries: Vec<FirewallPortEntry>,
    filter_port: Option<u16>,
) -> Vec<FirewallPortEntry> {
    let mut filtered = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in entries {
        if let Some(port) = filter_port {
            if !port_spec_matches(&entry.port, port) {
                continue;
            }
        }
        let key = (
            entry.port.clone(),
            entry.protocol.clone(),
            entry.family.clone(),
            entry.action.clone(),
        );
        if seen.insert(key) {
            filtered.push(entry);
        }
    }
    filtered.sort();
    filtered
}

fn parse_ufw_status(output: &str) -> Vec<FirewallPortEntry> {
    let mut entries = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("Status:")
            || trimmed.starts_with("To ")
            || trimmed.starts_with("--")
        {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let mut target = parts[0];
        let mut action_index = 1;
        let family = if parts.get(1) == Some(&"(v6)") {
            action_index = 2;
            "ipv6"
        } else {
            "ipv4"
        };

        let Some(action) = parts.get(action_index) else {
            continue;
        };
        if *action != "ALLOW" && *action != "LIMIT" {
            continue;
        }

        if let Some(stripped) = target.strip_suffix("(v6)") {
            target = stripped.trim();
        }
        let Some((port_spec, protocol)) = target.rsplit_once('/') else {
            continue;
        };

        for port in split_port_specs(port_spec) {
            entries.push(FirewallPortEntry {
                port,
                protocol: protocol.to_ascii_lowercase(),
                family: family.into(),
                action: (*action).into(),
            });
        }
    }

    entries
}

fn parse_nft_ruleset(output: &str) -> Vec<FirewallPortEntry> {
    let mut entries = Vec::new();
    let mut family = String::from("unknown");

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("table ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                family = parts[1].to_string();
            }
            continue;
        }
        if !trimmed.contains("accept") || !trimmed.contains("dport") {
            continue;
        }

        let protocol = if trimmed.starts_with("tcp ") || trimmed.contains(" tcp ") {
            "tcp"
        } else if trimmed.starts_with("udp ") || trimmed.contains(" udp ") {
            "udp"
        } else {
            continue;
        };

        let Some(specs) = extract_nft_port_specs(trimmed) else {
            continue;
        };

        for port in specs {
            entries.push(FirewallPortEntry {
                port,
                protocol: protocol.into(),
                family: family.clone(),
                action: "ACCEPT".into(),
            });
        }
    }

    entries
}

fn extract_nft_port_specs(line: &str) -> Option<Vec<String>> {
    let (_, tail) = line.split_once("dport")?;
    let tail = tail.trim_start();
    if let Some(rest) = tail.strip_prefix('{') {
        let end = rest.find('}')?;
        let inside = &rest[..end];
        let ports = inside
            .split(',')
            .filter_map(normalize_port_spec)
            .collect::<Vec<_>>();
        if ports.is_empty() {
            None
        } else {
            Some(ports)
        }
    } else {
        let token = tail.split_whitespace().next()?;
        normalize_port_spec(token).map(|item| vec![item])
    }
}

fn parse_iptables_rules(output: &str, family: &str) -> Vec<FirewallPortEntry> {
    let mut entries = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("-A INPUT ") || !trimmed.contains(" -j ACCEPT") {
            continue;
        }

        let Some(protocol) = extract_flag_value(trimmed, "-p") else {
            continue;
        };
        if protocol != "tcp" && protocol != "udp" {
            continue;
        }

        let port_specs = if let Some(spec) = extract_flag_value(trimmed, "--dport") {
            vec![spec.to_string()]
        } else if let Some(spec) = extract_flag_value(trimmed, "--dports") {
            spec.split(',').map(str::to_string).collect()
        } else {
            continue;
        };

        for spec in port_specs {
            if let Some(port) = normalize_port_spec(&spec) {
                entries.push(FirewallPortEntry {
                    port,
                    protocol: protocol.into(),
                    family: family.into(),
                    action: "ACCEPT".into(),
                });
            }
        }
    }

    entries
}

fn extract_flag_value<'a>(line: &'a str, flag: &str) -> Option<&'a str> {
    let marker = format!("{} ", flag);
    let (_, tail) = line.split_once(&marker)?;
    tail.split_whitespace().next()
}

fn split_port_specs(value: &str) -> Vec<String> {
    value.split(',').filter_map(normalize_port_spec).collect()
}

fn normalize_port_spec(value: &str) -> Option<String> {
    let normalized = value.trim().trim_matches(',').replace(':', "-");
    if normalized.is_empty() {
        return None;
    }
    if normalized
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch == '-')
    {
        Some(normalized)
    } else {
        None
    }
}

fn port_spec_matches(spec: &str, port: u16) -> bool {
    if let Some((start, end)) = spec.split_once('-') {
        let Ok(start) = start.parse::<u16>() else {
            return false;
        };
        let Ok(end) = end.parse::<u16>() else {
            return false;
        };
        return start <= port && port <= end;
    }

    spec.parse::<u16>()
        .map(|value| value == port)
        .unwrap_or(false)
}

fn emit_success(
    format: OutputFormat,
    data: &FirewallPortsOutput,
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
                println!();
                for item in &report.warnings {
                    println!("警告: {}", item);
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

fn render_plain_tsv(data: &FirewallPortsOutput) -> String {
    let mut lines = vec!["port\tprotocol\tfamily\taction\tbackend".to_string()];
    for entry in &data.entries {
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}",
            entry.port, entry.protocol, entry.family, entry.action, data.backend
        ));
    }
    lines.join("\n")
}

fn render_human_table(data: &FirewallPortsOutput) -> String {
    let mut lines = vec![
        format!("防火墙: {}", data.backend),
        format!(
            "筛选端口: {}",
            data.filter_port
                .map(|port| port.to_string())
                .unwrap_or_else(|| "全部".to_string())
        ),
        format!("匹配规则: {}", data.entry_count),
    ];
    lines.push(render_entries_table(&data.entries));
    lines.join("\n")
}

fn render_entries_table(entries: &[FirewallPortEntry]) -> String {
    let port_width = entries
        .iter()
        .map(|item| item.port.len())
        .max()
        .unwrap_or(2)
        .max("端口".len());
    let protocol_width = entries
        .iter()
        .map(|item| item.protocol.len())
        .max()
        .unwrap_or(2)
        .max("协议".len());
    let family_width = entries
        .iter()
        .map(|item| item.family.len())
        .max()
        .unwrap_or(2)
        .max("地址族".len());
    let action_width = entries
        .iter()
        .map(|item| item.action.len())
        .max()
        .unwrap_or(2)
        .max("动作".len());

    let header = format!(
        "{:<port_width$}  {:<protocol_width$}  {:<family_width$}  {:<action_width$}",
        "端口", "协议", "地址族", "动作"
    );
    let separator = format!(
        "{:-<port_width$}  {:-<protocol_width$}  {:-<family_width$}  {:-<action_width$}",
        "", "", "", ""
    );

    let mut lines = vec![header, separator];
    if entries.is_empty() {
        lines.push("未匹配到开放规则".into());
        return lines.join("\n");
    }

    for entry in entries {
        lines.push(format!(
            "{:<port_width$}  {:<protocol_width$}  {:<family_width$}  {:<action_width$}",
            entry.port, entry.protocol, entry.family, entry.action
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ufw_status_supports_multiport_and_ipv6() {
        let output = r#"Status: active

To                         Action      From
--                         ------      ----
22/tcp                     ALLOW       Anywhere
80,443/tcp                 ALLOW       Anywhere
60000:61000/udp            ALLOW       Anywhere
22/tcp (v6)                ALLOW       Anywhere (v6)
"#;

        let entries = parse_ufw_status(output);
        assert!(entries.contains(&FirewallPortEntry {
            port: "22".into(),
            protocol: "tcp".into(),
            family: "ipv4".into(),
            action: "ALLOW".into(),
        }));
        assert!(entries.contains(&FirewallPortEntry {
            port: "443".into(),
            protocol: "tcp".into(),
            family: "ipv4".into(),
            action: "ALLOW".into(),
        }));
        assert!(entries.contains(&FirewallPortEntry {
            port: "60000-61000".into(),
            protocol: "udp".into(),
            family: "ipv4".into(),
            action: "ALLOW".into(),
        }));
        assert!(entries.contains(&FirewallPortEntry {
            port: "22".into(),
            protocol: "tcp".into(),
            family: "ipv6".into(),
            action: "ALLOW".into(),
        }));
    }

    #[test]
    fn parse_nft_ruleset_supports_sets_and_ranges() {
        let output = r#"table inet filter {
    chain input {
        tcp dport 22 accept
        udp dport { 53, 67 } accept
        tcp dport 60000-61000 accept
    }
}"#;

        let entries = parse_nft_ruleset(output);
        assert!(entries.contains(&FirewallPortEntry {
            port: "22".into(),
            protocol: "tcp".into(),
            family: "inet".into(),
            action: "ACCEPT".into(),
        }));
        assert!(entries.contains(&FirewallPortEntry {
            port: "53".into(),
            protocol: "udp".into(),
            family: "inet".into(),
            action: "ACCEPT".into(),
        }));
        assert!(entries.contains(&FirewallPortEntry {
            port: "60000-61000".into(),
            protocol: "tcp".into(),
            family: "inet".into(),
            action: "ACCEPT".into(),
        }));
    }

    #[test]
    fn parse_iptables_rules_supports_multiport_and_filtering() {
        let output = r#"-P INPUT ACCEPT
-A INPUT -p tcp -m tcp --dport 22 -j ACCEPT
-A INPUT -p tcp -m multiport --dports 80,443 -j ACCEPT
-A INPUT -p udp -m udp --dport 60000:61000 -j ACCEPT
"#;

        let filtered = filter_entries(parse_iptables_rules(output, "ipv4"), Some(443));
        assert_eq!(
            filtered,
            vec![FirewallPortEntry {
                port: "443".into(),
                protocol: "tcp".into(),
                family: "ipv4".into(),
                action: "ACCEPT".into(),
            }]
        );

        let range_filtered = filter_entries(parse_iptables_rules(output, "ipv4"), Some(60001));
        assert_eq!(
            range_filtered,
            vec![FirewallPortEntry {
                port: "60000-61000".into(),
                protocol: "udp".into(),
                family: "ipv4".into(),
                action: "ACCEPT".into(),
            }]
        );
    }

    #[test]
    fn render_human_table_contains_summary_and_rows() {
        let data = FirewallPortsOutput {
            backend: "ufw".into(),
            filter_port: Some(443),
            entry_count: 2,
            entries: vec![
                FirewallPortEntry {
                    port: "22".into(),
                    protocol: "tcp".into(),
                    family: "ipv4".into(),
                    action: "ALLOW".into(),
                },
                FirewallPortEntry {
                    port: "443".into(),
                    protocol: "tcp".into(),
                    family: "ipv6".into(),
                    action: "ALLOW".into(),
                },
            ],
        };

        let rendered = render_human_table(&data);
        assert!(rendered.contains("防火墙: ufw"));
        assert!(rendered.contains("筛选端口: 443"));
        assert!(rendered.contains("匹配规则: 2"));
        assert!(rendered.contains("端口"));
        assert!(rendered.contains("443"));
        assert!(rendered.contains("ipv6"));
    }

    #[test]
    fn render_entries_table_handles_empty_rows() {
        let rendered = render_entries_table(&[]);
        assert!(rendered.contains("端口"));
        assert!(rendered.contains("未匹配到开放规则"));
    }

    #[test]
    fn render_plain_tsv_uses_header_and_tab_separators() {
        let data = FirewallPortsOutput {
            backend: "ufw".into(),
            filter_port: None,
            entry_count: 1,
            entries: vec![FirewallPortEntry {
                port: "443".into(),
                protocol: "tcp".into(),
                family: "ipv4".into(),
                action: "ALLOW".into(),
            }],
        };

        let rendered = render_plain_tsv(&data);
        assert!(rendered.starts_with("port\tprotocol\tfamily\taction\tbackend"));
        assert!(rendered.contains("\n443\ttcp\tipv4\tALLOW\tufw"));
        assert!(!rendered.contains("filter_port:"));
    }

    #[test]
    fn render_plain_tsv_keeps_header_when_empty() {
        let data = FirewallPortsOutput {
            backend: "iptables".into(),
            filter_port: Some(1234),
            entry_count: 0,
            entries: vec![],
        };

        assert_eq!(
            render_plain_tsv(&data),
            "port\tprotocol\tfamily\taction\tbackend"
        );
    }
}
