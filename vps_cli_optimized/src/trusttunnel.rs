use clap::{Arg, ArgAction, ArgMatches, Command, ValueHint};
use dialoguer::Input;
use serde_json::{json, Value as JsonValue};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as SysCmd;

use crate::reclaim::{trusttunnel_purge_shared, trusttunnel_uninstall_shared};
use crate::safety::{current_timestamp, ensure_dir, is_interactive, require_confirmation};
use crate::utils::{Breadcrumb, CliError, OperationReport, OutputEnvelope, OutputFormat};

const TRUSTTUNNEL_INSTALLER_URL: &str =
    "https://raw.githubusercontent.com/TrustTunnel/TrustTunnel/refs/heads/master/scripts/install.sh";
const TRUSTTUNNEL_DEFAULT_INSTALL_DIR: &str = "/opt/trusttunnel";
const TRUSTTUNNEL_BINARY_NAME: &str = "trusttunnel_endpoint";
const TRUSTTUNNEL_WIZARD_NAME: &str = "setup_wizard";
const TRUSTTUNNEL_SERVICE_TEMPLATE: &str = "trusttunnel.service.template";
const TRUSTTUNNEL_SERVICE_FILE: &str = "/etc/systemd/system/trusttunnel.service";
const TRUSTTUNNEL_SERVICE_NAME: &str = "trusttunnel";

pub fn cli() -> Command {
    Command::new("trusttunnel")
        .about("管理 TrustTunnel 服务端安装、配置导出与 systemd 服务")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("install")
                .about("安装或更新 TrustTunnel 服务端")
                .arg(
                    Arg::new("version")
                        .long("version")
                        .value_name("VERSION")
                        .help("指定 TrustTunnel 版本，例如 1.0.33"),
                )
                .arg(
                    Arg::new("output-dir")
                        .long("output-dir")
                        .value_name("DIR")
                        .value_hint(ValueHint::DirPath)
                        .help("安装目录，默认 /opt/trusttunnel"),
                )
                .arg(
                    Arg::new("auto-yes")
                        .long("auto-yes")
                        .action(ArgAction::SetTrue)
                        .help("向官方安装脚本自动回答 yes"),
                )
                .arg(
                    Arg::new("dry-run")
                        .long("dry-run")
                        .action(ArgAction::SetTrue)
                        .help("仅预览将执行的安装动作"),
                )
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认，直接安装或更新"),
                ),
        )
        .subcommand(Command::new("status").about("查看 TrustTunnel 服务状态"))
        .subcommand(Command::new("start").about("启动 TrustTunnel 服务"))
        .subcommand(Command::new("stop").about("停止 TrustTunnel 服务"))
        .subcommand(Command::new("restart").about("重启 TrustTunnel 服务"))
        .subcommand(
            Command::new("logs")
                .about("查看 TrustTunnel 服务日志")
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
            Command::new("setup-wizard")
                .about("执行官方 setup_wizard 配置向导")
                .arg(
                    Arg::new("install-dir")
                        .long("install-dir")
                        .value_name("DIR")
                        .value_hint(ValueHint::DirPath)
                        .help("TrustTunnel 安装目录，默认 /opt/trusttunnel"),
                )
                .arg(
                    Arg::new("print-paths")
                        .long("print-paths")
                        .action(ArgAction::SetTrue)
                        .help("只显示默认配置文件路径，不执行向导"),
                )
                .arg(
                    Arg::new("wizard-args")
                        .help("透传给 setup_wizard 的额外参数；如需透传请在 -- 后书写")
                        .num_args(0..)
                        .trailing_var_arg(true)
                        .allow_hyphen_values(true),
                ),
        )
        .subcommand(
            Command::new("export-config")
                .about("导出 TrustTunnel 客户端配置")
                .arg(
                    Arg::new("install-dir")
                        .long("install-dir")
                        .value_name("DIR")
                        .value_hint(ValueHint::DirPath)
                        .help("TrustTunnel 安装目录，默认 /opt/trusttunnel"),
                )
                .arg(
                    Arg::new("settings")
                        .long("settings")
                        .value_name("FILE")
                        .value_hint(ValueHint::FilePath)
                        .help("主配置文件路径，默认 <install-dir>/vpn.toml"),
                )
                .arg(
                    Arg::new("hosts")
                        .long("hosts")
                        .value_name("FILE")
                        .value_hint(ValueHint::FilePath)
                        .help("TLS host 配置文件路径，默认 <install-dir>/hosts.toml"),
                )
                .arg(
                    Arg::new("client")
                        .long("client")
                        .value_name("NAME")
                        .help("导出的客户端名称，映射 trusttunnel_endpoint -c"),
                )
                .arg(
                    Arg::new("address")
                        .long("address")
                        .value_name("ADDR")
                        .help("客户端连接地址，映射 trusttunnel_endpoint -a"),
                )
                .arg(
                    Arg::new("format")
                        .long("format")
                        .value_name("FORMAT")
                        .default_value("deeplink")
                        .value_parser(["deeplink", "toml"])
                        .help("导出格式：deeplink 或 toml"),
                )
                .arg(
                    Arg::new("name")
                        .long("name")
                        .value_name("DISPLAY_NAME")
                        .help("客户端显示名称"),
                )
                .arg(
                    Arg::new("dns-upstream")
                        .long("dns-upstream")
                        .value_name("UPSTREAM")
                        .action(ArgAction::Append)
                        .help("附加 DNS upstream，可重复传入"),
                )
                .arg(
                    Arg::new("generate-client-random-prefix")
                        .long("generate-client-random-prefix")
                        .action(ArgAction::SetTrue)
                        .help("生成新的 client random prefix 并写回 rules.toml"),
                )
                .arg(
                    Arg::new("client-random-prefix")
                        .long("client-random-prefix")
                        .value_name("PREFIX")
                        .help("显式指定 client random prefix"),
                )
                .arg(
                    Arg::new("prefix-length")
                        .long("prefix-length")
                        .value_name("N")
                        .help("生成前缀时使用的长度"),
                )
                .arg(
                    Arg::new("prefix-percent")
                        .long("prefix-percent")
                        .value_name("N")
                        .help("生成前缀时 one bit 百分比"),
                )
                .arg(
                    Arg::new("prefix-mask")
                        .long("prefix-mask")
                        .value_name("HEX")
                        .help("生成前缀时使用的显式 mask"),
                )
                .arg(
                    Arg::new("show-secrets")
                        .long("show-secrets")
                        .action(ArgAction::SetTrue)
                        .help("显示完整导出结果，包含敏感 deeplink 或 TOML"),
                ),
        )
        .subcommand(
            Command::new("uninstall")
                .about("卸载 TrustTunnel 服务端安装目录内容")
                .arg(
                    Arg::new("output-dir")
                        .long("output-dir")
                        .value_name("DIR")
                        .value_hint(ValueHint::DirPath)
                        .help("TrustTunnel 安装目录，默认 /opt/trusttunnel"),
                )
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认，直接卸载"),
                ),
        )
        .subcommand(
            Command::new("purge")
                .about("彻底清理 TrustTunnel 安装目录与 systemd 文件")
                .arg(
                    Arg::new("output-dir")
                        .long("output-dir")
                        .value_name("DIR")
                        .value_hint(ValueHint::DirPath)
                        .help("TrustTunnel 安装目录，默认 /opt/trusttunnel"),
                )
                .arg(
                    Arg::new("confirm")
                        .long("confirm")
                        .action(ArgAction::SetTrue)
                        .help("跳过交互确认，直接清理"),
                ),
        )
}

pub fn handle_trusttunnel(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    match matches.subcommand() {
        Some(("install", sub)) => install_trusttunnel(sub, format, no_input),
        Some(("status", _)) => service_status(format),
        Some(("start", _)) => service_action("start", format),
        Some(("stop", _)) => service_action("stop", format),
        Some(("restart", _)) => service_action("restart", format),
        Some(("logs", sub)) => show_logs(sub, format, no_input),
        Some(("setup-wizard", sub)) => run_setup_wizard(sub, format, no_input),
        Some(("export-config", sub)) => export_config(sub, format, no_input),
        Some(("uninstall", sub)) => uninstall_trusttunnel(sub, format, no_input),
        Some(("purge", sub)) => purge_trusttunnel(sub, format, no_input),
        _ => Err(CliError::new("未知子命令")),
    }
}

fn install_trusttunnel(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let version = matches.get_one::<String>("version").cloned();
    let output_dir = normalized_install_dir(matches.get_one::<String>("output-dir"));
    let auto_yes = matches.get_flag("auto-yes");
    let dry_run = matches.get_flag("dry-run");
    let confirm = matches.get_flag("confirm");
    let interactive = is_interactive(no_input);
    require_confirmation(
        "确认安装或更新 TrustTunnel 服务端？",
        interactive,
        dry_run,
        confirm,
    )?;

    let mut installer_args = installer_common_args(&output_dir, version.as_deref());
    let implied_auto_yes = auto_yes || !interactive;
    if implied_auto_yes {
        installer_args.push("-a".into());
        installer_args.push("y".into());
    }

    let existing = trusttunnel_binary_path(&output_dir).exists();
    if dry_run {
        emit_success(
            format,
            json!({
                "dry_run": true,
                "installer": "official-script",
                "installer_url": TRUSTTUNNEL_INSTALLER_URL,
                "version": version,
                "output_dir": output_dir,
                "existing_installation": existing,
                "script_args": installer_args,
            }),
            &OperationReport::default(),
            Some(vec![
                crumb("执行安装", "vps-cli trusttunnel install --confirm"),
                crumb("查看状态", "vps-cli trusttunnel status"),
            ]),
        );
        return Ok(());
    }

    let tmp_dir = env::temp_dir().join(format!(
        "vps-cli-trusttunnel-install-{}",
        current_timestamp()
    ));
    ensure_dir(&tmp_dir)?;
    let script_path = tmp_dir.join("install.sh");
    download_file(TRUSTTUNNEL_INSTALLER_URL, &script_path)?;
    let output = SysCmd::new("sh")
        .arg(&script_path)
        .args(installer_args.iter().map(String::as_str))
        .output()
        .map_err(|e| CliError::new(format!("执行 TrustTunnel 官方安装脚本失败: {}", e)))?;
    if !output.status.success() {
        return Err(command_failed(
            "TrustTunnel 安装失败",
            &output.stdout,
            &output.stderr,
        ));
    }

    let mut report = OperationReport::default();
    report.changed_files.push(output_dir.clone());
    if !interactive && !auto_yes {
        report
            .warnings
            .push("当前为非交互模式，已自动向官方安装脚本传入 `-a y`。".into());
    }

    let wizard_path = trusttunnel_wizard_path(&output_dir);
    let data = json!({
        "installed": true,
        "installer": "official-script",
        "version": version,
        "output_dir": output_dir,
        "existing_installation": existing,
        "detected_binaries": detected_binaries(&output_dir),
        "service_template": trusttunnel_service_template_path(&output_dir).display().to_string(),
        "setup_wizard": wizard_path.display().to_string(),
    });
    emit_success(
        format,
        data,
        &report,
        Some(vec![
            crumb("执行配置向导", "vps-cli trusttunnel setup-wizard"),
            crumb("查看服务状态", "vps-cli trusttunnel status"),
        ]),
    );
    Ok(())
}

fn service_status(format: OutputFormat) -> Result<(), CliError> {
    let output_dir = TRUSTTUNNEL_DEFAULT_INSTALL_DIR.to_string();
    let binary_exists = trusttunnel_binary_path(&output_dir).exists();
    let wizard_exists = trusttunnel_wizard_path(&output_dir).exists();
    let service_exists = Path::new(TRUSTTUNNEL_SERVICE_FILE).exists();
    let status = SysCmd::new("systemctl")
        .args(["is-active", TRUSTTUNNEL_SERVICE_NAME])
        .output();
    let (service_status, stderr) = match status {
        Ok(output) => {
            let service_status = if output.status.success() {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            } else {
                "unknown".into()
            };
            (
                service_status,
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            )
        }
        Err(err) => ("unknown".into(), err.to_string()),
    };
    let mut report = OperationReport::default();
    if !stderr.is_empty() {
        report.warnings.push(stderr);
    }
    if !service_exists {
        report.warnings.push(missing_service_guidance(&output_dir));
    }
    emit_success(
        format,
        json!({
            "service": TRUSTTUNNEL_SERVICE_NAME,
            "status": service_status,
            "install_dir": output_dir,
            "binary_exists": binary_exists,
            "setup_wizard_exists": wizard_exists,
            "service_file_exists": service_exists,
        }),
        &report,
        Some(vec![
            crumb("查看日志", "vps-cli trusttunnel logs --lines 100"),
            crumb("执行向导", "vps-cli trusttunnel setup-wizard"),
        ]),
    );
    Ok(())
}

fn service_action(action: &str, format: OutputFormat) -> Result<(), CliError> {
    if !Path::new(TRUSTTUNNEL_SERVICE_FILE).exists() {
        return Err(CliError::new(missing_service_guidance(
            TRUSTTUNNEL_DEFAULT_INSTALL_DIR,
        )));
    }
    let output = SysCmd::new("systemctl")
        .args([action, TRUSTTUNNEL_SERVICE_NAME])
        .output()
        .map_err(|e| CliError::new(format!("执行 systemctl {} 失败: {}", action, e)))?;
    if !output.status.success() {
        return Err(command_failed(
            &format!("TrustTunnel 服务 {} 失败", action),
            &output.stdout,
            &output.stderr,
        ));
    }
    emit_success(
        format,
        json!({action: true, "service": TRUSTTUNNEL_SERVICE_NAME}),
        &OperationReport::default(),
        Some(vec![crumb("查看状态", "vps-cli trusttunnel status")]),
    );
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
    cmd.args(["-u", TRUSTTUNNEL_SERVICE_NAME, "-n", &lines, "--no-pager"]);
    if follow {
        cmd.arg("-f");
    }
    let output = cmd
        .output()
        .map_err(|e| CliError::new(format!("读取 TrustTunnel 日志失败: {}", e)))?;
    if !output.status.success() {
        return Err(command_failed(
            "读取 TrustTunnel 日志失败",
            &output.stdout,
            &output.stderr,
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    match format {
        OutputFormat::Json => emit_success(
            format,
            json!({"service": TRUSTTUNNEL_SERVICE_NAME, "logs": text}),
            &OperationReport::default(),
            None,
        ),
        _ => print!("{}", text),
    }
    Ok(())
}

fn run_setup_wizard(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let install_dir = normalized_install_dir(matches.get_one::<String>("install-dir"));
    if matches.get_flag("print-paths") {
        emit_success(
            format,
            config_path_summary(&install_dir),
            &OperationReport::default(),
            Some(vec![crumb("执行向导", "vps-cli trusttunnel setup-wizard")]),
        );
        return Ok(());
    }

    let wizard_path = trusttunnel_wizard_path(&install_dir);
    if !wizard_path.exists() {
        return Err(CliError::new(format!(
            "未找到 setup_wizard，请先执行 `vps-cli trusttunnel install`：{}",
            wizard_path.display()
        )));
    }
    let extra_args = matches
        .get_many::<String>("wizard-args")
        .map(|values| values.cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let interactive = is_interactive(no_input);
    if !interactive && extra_args.is_empty() {
        return Err(CliError::new(
            "非交互模式下执行 setup-wizard 需要在 `--` 后透传官方非交互参数",
        ));
    }

    if interactive && !matches!(format, OutputFormat::Json) {
        let status = SysCmd::new(&wizard_path)
            .args(extra_args.iter().map(String::as_str))
            .current_dir(&install_dir)
            .status()
            .map_err(|e| CliError::new(format!("执行 setup_wizard 失败: {}", e)))?;
        if !status.success() {
            return Err(CliError::new("setup_wizard 执行失败"));
        }
        let service_installed =
            ensure_trusttunnel_service_installed(&install_dir).map_err(|err| {
                CliError::new(format!(
                    "setup_wizard 已完成，但安装 systemd 服务文件失败: {}",
                    err
                ))
            })?;
        emit_success(
            format,
            json!({
                "ran": true,
                "interactive": true,
                "install_dir": install_dir,
                "config_paths": config_path_summary(&install_dir),
                "systemd_service": TRUSTTUNNEL_SERVICE_FILE,
                "service_file_installed": service_installed || Path::new(TRUSTTUNNEL_SERVICE_FILE).exists(),
            }),
            &OperationReport::default(),
            Some(vec![
                crumb("启动服务", "vps-cli trusttunnel start"),
                crumb("导出配置", "vps-cli trusttunnel export-config --help"),
            ]),
        );
        return Ok(());
    }

    let output = SysCmd::new(&wizard_path)
        .args(extra_args.iter().map(String::as_str))
        .current_dir(&install_dir)
        .output()
        .map_err(|e| CliError::new(format!("执行 setup_wizard 失败: {}", e)))?;
    if !output.status.success() {
        return Err(command_failed(
            "setup_wizard 执行失败",
            &output.stdout,
            &output.stderr,
        ));
    }
    let service_installed = ensure_trusttunnel_service_installed(&install_dir).map_err(|err| {
        CliError::new(format!(
            "setup_wizard 已完成，但安装 systemd 服务文件失败: {}",
            err
        ))
    })?;
    emit_success(
        format,
        json!({
            "ran": true,
            "interactive": false,
            "install_dir": install_dir,
            "config_paths": config_path_summary(&install_dir),
            "systemd_service": TRUSTTUNNEL_SERVICE_FILE,
            "service_file_installed": service_installed || Path::new(TRUSTTUNNEL_SERVICE_FILE).exists(),
            "stdout": non_empty_text(&output.stdout),
        }),
        &OperationReport::default(),
        Some(vec![
            crumb("启动服务", "vps-cli trusttunnel start"),
            crumb("导出配置", "vps-cli trusttunnel export-config --help"),
        ]),
    );
    Ok(())
}

fn export_config(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let interactive = is_interactive(no_input);
    let install_dir = normalized_install_dir(matches.get_one::<String>("install-dir"));
    let binary_path = trusttunnel_binary_path(&install_dir);
    if !binary_path.exists() {
        return Err(CliError::new(format!(
            "未找到 trusttunnel_endpoint，请先执行 `vps-cli trusttunnel install`：{}",
            binary_path.display()
        )));
    }
    let settings = matches
        .get_one::<String>("settings")
        .cloned()
        .unwrap_or_else(|| {
            Path::new(&install_dir)
                .join("vpn.toml")
                .display()
                .to_string()
        });
    let hosts = matches
        .get_one::<String>("hosts")
        .cloned()
        .unwrap_or_else(|| {
            Path::new(&install_dir)
                .join("hosts.toml")
                .display()
                .to_string()
        });
    let inferred_client = matches
        .get_one::<String>("client")
        .cloned()
        .or_else(|| infer_first_client_name(&settings).ok().flatten());
    let inferred_address = matches
        .get_one::<String>("address")
        .cloned()
        .or_else(|| infer_default_address(&hosts).ok().flatten());
    let client = required_or_prompt(inferred_client, "请输入导出的客户端名称", interactive)?;
    let address = required_or_prompt(
        inferred_address,
        "请输入客户端连接地址（优先域名，可带端口）",
        interactive,
    )?;
    let export_format = matches.get_one::<String>("format").unwrap().clone();
    let show_secrets = matches.get_flag("show-secrets");

    let mut args = vec![
        settings.clone(),
        hosts.clone(),
        "-c".into(),
        client.clone(),
        "-a".into(),
        address.clone(),
        "--format".into(),
        export_format.clone(),
    ];
    if let Some(name) = matches.get_one::<String>("name") {
        args.push("--name".into());
        args.push(name.clone());
    }
    if let Some(values) = matches.get_many::<String>("dns-upstream") {
        for value in values {
            args.push("--dns-upstream".into());
            args.push(value.clone());
        }
    }
    if matches.get_flag("generate-client-random-prefix") {
        args.push("--generate-client-random-prefix".into());
    }
    if let Some(prefix) = matches.get_one::<String>("client-random-prefix") {
        args.push("--client-random-prefix".into());
        args.push(prefix.clone());
    }
    if let Some(value) = matches.get_one::<String>("prefix-length") {
        args.push("--prefix-length".into());
        args.push(value.clone());
    }
    if let Some(value) = matches.get_one::<String>("prefix-percent") {
        args.push("--prefix-percent".into());
        args.push(value.clone());
    }
    if let Some(value) = matches.get_one::<String>("prefix-mask") {
        args.push("--prefix-mask".into());
        args.push(value.clone());
    }

    let output = SysCmd::new(&binary_path)
        .args(args.iter().map(String::as_str))
        .current_dir(&install_dir)
        .output()
        .map_err(|e| CliError::new(format!("执行 trusttunnel_endpoint 导出配置失败: {}", e)))?;
    if !output.status.success() {
        return Err(command_failed(
            "TrustTunnel 导出配置失败",
            &output.stdout,
            &output.stderr,
        ));
    }

    let mut data = json!({
        "exported": true,
        "install_dir": install_dir,
        "settings": settings,
        "hosts": hosts,
        "client": client,
        "address": address,
        "format": export_format,
        "show_secrets": show_secrets,
        "tips": [
            "deeplink 格式会默认直接输出链接，便于客户端导入",
            "如果导出的是 deeplink 或 TOML，请避免贴入公开日志"
        ]
    });
    let mut report = OperationReport::default();
    let exported_content = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if show_secrets {
        data["content"] = JsonValue::String(exported_content.clone());
        report.sensitive = Some(true);
        report
            .warnings
            .push("当前输出包含敏感配置内容，不应贴入公开日志。".into());
    } else if should_inline_deeplink(&export_format) {
        data["link"] = JsonValue::String(exported_content);
        report.sensitive = Some(true);
        report
            .warnings
            .push("当前输出已直接包含 TrustTunnel deeplink，请勿贴入公开日志。".into());
    } else {
        data["content_hidden"] = JsonValue::Bool(true);
        data["stdout_preview"] = JsonValue::String("已隐藏，请显式传入 --show-secrets 查看".into());
    }
    emit_success(
        format,
        data,
        &report,
        Some(vec![
            crumb("查看状态", "vps-cli trusttunnel status"),
            crumb(
                "重新导出并显示内容",
                "vps-cli trusttunnel export-config --show-secrets",
            ),
        ]),
    );
    Ok(())
}

fn uninstall_trusttunnel(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let output_dir = normalized_install_dir(matches.get_one::<String>("output-dir"));
    let interactive = is_interactive(no_input);
    let confirm = matches.get_flag("confirm");
    trusttunnel_uninstall_shared(&output_dir, format, interactive, confirm)
}

fn purge_trusttunnel(
    matches: &ArgMatches,
    format: OutputFormat,
    no_input: bool,
) -> Result<(), CliError> {
    let output_dir = normalized_install_dir(matches.get_one::<String>("output-dir"));
    let interactive = is_interactive(no_input);
    let confirm = matches.get_flag("confirm");
    trusttunnel_purge_shared(&output_dir, format, interactive, confirm)
}

fn normalized_install_dir(value: Option<&String>) -> String {
    let raw = value
        .map(|s| s.trim())
        .unwrap_or(TRUSTTUNNEL_DEFAULT_INSTALL_DIR);
    match raw {
        "" | "." | "/opt" => TRUSTTUNNEL_DEFAULT_INSTALL_DIR.to_string(),
        _ => raw.to_string(),
    }
}

fn installer_common_args(output_dir: &str, version: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    args.push("-o".into());
    args.push(output_dir.into());
    if let Some(version) = version {
        args.push("-V".into());
        args.push(version.into());
    }
    args
}

fn trusttunnel_binary_path(install_dir: &str) -> PathBuf {
    Path::new(install_dir).join(TRUSTTUNNEL_BINARY_NAME)
}

fn trusttunnel_wizard_path(install_dir: &str) -> PathBuf {
    Path::new(install_dir).join(TRUSTTUNNEL_WIZARD_NAME)
}

fn trusttunnel_service_template_path(install_dir: &str) -> PathBuf {
    Path::new(install_dir).join(TRUSTTUNNEL_SERVICE_TEMPLATE)
}

fn infer_first_client_name(settings_path: &str) -> Result<Option<String>, CliError> {
    let credentials_path = resolve_credentials_path(settings_path)?;
    first_toml_string_value(&credentials_path, "username")
}

fn infer_default_address(hosts_path: &str) -> Result<Option<String>, CliError> {
    first_toml_string_value(Path::new(hosts_path), "hostname")
}

fn resolve_credentials_path(settings_path: &str) -> Result<PathBuf, CliError> {
    let settings_path = Path::new(settings_path);
    let base_dir = settings_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let configured = first_toml_string_value(settings_path, "credentials_file")?;
    Ok(match configured {
        Some(path) => {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                base_dir.join(path)
            }
        }
        None => base_dir.join("credentials.toml"),
    })
}

fn first_toml_string_value(path: &Path, key: &str) -> Result<Option<String>, CliError> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)
        .map_err(|e| CliError::new(format!("读取配置文件失败 {}: {}", path.display(), e)))?;
    for line in content.lines() {
        if let Some(value) = parse_toml_string_assignment(line, key) {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn parse_toml_string_assignment(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let (lhs, rhs) = trimmed.split_once('=')?;
    if lhs.trim() != key {
        return None;
    }
    parse_toml_string_literal(rhs.trim())
}

fn parse_toml_string_literal(value: &str) -> Option<String> {
    let value = value.strip_prefix('"')?;
    let end = value.find('"')?;
    Some(value[..end].to_string())
}

fn should_inline_deeplink(export_format: &str) -> bool {
    export_format == "deeplink"
}

fn sync_service_template(template_path: &Path, service_path: &Path) -> Result<bool, CliError> {
    if !template_path.exists() {
        return Err(CliError::new(format!(
            "未找到 TrustTunnel systemd 模板：{}",
            template_path.display()
        )));
    }
    if let Some(parent) = service_path.parent() {
        ensure_dir(parent)?;
    }
    let template_content = fs::read(template_path).map_err(|e| {
        CliError::new(format!(
            "读取 TrustTunnel systemd 模板失败 {}: {}",
            template_path.display(),
            e
        ))
    })?;
    if service_path.exists() {
        let existing_content = fs::read(service_path).map_err(|e| {
            CliError::new(format!(
                "读取现有 TrustTunnel systemd 文件失败 {}: {}",
                service_path.display(),
                e
            ))
        })?;
        if existing_content == template_content {
            return Ok(false);
        }
    }
    fs::write(service_path, template_content).map_err(|e| {
        CliError::new(format!(
            "写入 TrustTunnel systemd 文件失败 {}: {}",
            service_path.display(),
            e
        ))
    })?;
    Ok(true)
}

fn daemon_reload() -> Result<(), CliError> {
    let output = SysCmd::new("systemctl")
        .arg("daemon-reload")
        .output()
        .map_err(|e| CliError::new(format!("执行 systemctl daemon-reload 失败: {}", e)))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_failed(
            "systemctl daemon-reload 失败",
            &output.stdout,
            &output.stderr,
        ))
    }
}

fn ensure_trusttunnel_service_installed(install_dir: &str) -> Result<bool, CliError> {
    let changed = sync_service_template(
        &trusttunnel_service_template_path(install_dir),
        Path::new(TRUSTTUNNEL_SERVICE_FILE),
    )?;
    if changed {
        daemon_reload()?;
    }
    Ok(changed)
}

fn missing_service_guidance(install_dir: &str) -> String {
    let template = trusttunnel_service_template_path(install_dir);
    if template.exists() {
        format!(
            "未检测到 systemd 服务文件 {}。请先执行 `vps-cli trusttunnel setup-wizard`，CLI 会将模板 {} 安装为 systemd 服务。",
            TRUSTTUNNEL_SERVICE_FILE,
            template.display()
        )
    } else {
        format!(
            "未检测到 systemd 服务文件 {}，且未找到服务模板 {}。请先执行 `vps-cli trusttunnel install`，再执行 `vps-cli trusttunnel setup-wizard`。",
            TRUSTTUNNEL_SERVICE_FILE,
            template.display()
        )
    }
}

fn detected_binaries(install_dir: &str) -> Vec<String> {
    [
        trusttunnel_binary_path(install_dir),
        trusttunnel_wizard_path(install_dir),
        trusttunnel_service_template_path(install_dir),
    ]
    .iter()
    .filter(|path| path.exists())
    .map(|path| path.display().to_string())
    .collect()
}

fn config_path_summary(install_dir: &str) -> JsonValue {
    json!({
        "vpn": Path::new(install_dir).join("vpn.toml").display().to_string(),
        "hosts": Path::new(install_dir).join("hosts.toml").display().to_string(),
        "credentials": Path::new(install_dir).join("credentials.toml").display().to_string(),
        "rules": Path::new(install_dir).join("rules.toml").display().to_string(),
        "service_template": trusttunnel_service_template_path(install_dir).display().to_string(),
        "systemd_service": TRUSTTUNNEL_SERVICE_FILE,
    })
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

fn non_empty_text(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
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

fn command_failed(prefix: &str, stdout: &[u8], stderr: &[u8]) -> CliError {
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "命令未返回更多信息".into()
    };
    CliError::new(format!("{}: {}", prefix, detail))
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
    use super::{
        infer_default_address, infer_first_client_name, parse_toml_string_assignment,
        resolve_credentials_path, should_inline_deeplink, sync_service_template,
    };
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(name: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!(
            "vps-cli-trusttunnel-tests-{}-{}-{}",
            name,
            process::id(),
            ts
        ))
    }

    #[test]
    fn sync_service_template_creates_service_file() {
        let dir = temp_test_dir("create");
        fs::create_dir_all(&dir).unwrap();
        let template = dir.join("trusttunnel.service.template");
        let service = dir.join("systemd").join("trusttunnel.service");
        fs::write(&template, "[Unit]\nDescription=TrustTunnel\n").unwrap();

        let changed = sync_service_template(&template, &service).unwrap();

        assert!(changed);
        assert_eq!(
            fs::read_to_string(&service).unwrap(),
            "[Unit]\nDescription=TrustTunnel\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_service_template_skips_unchanged_service_file() {
        let dir = temp_test_dir("unchanged");
        fs::create_dir_all(dir.join("systemd")).unwrap();
        let template = dir.join("trusttunnel.service.template");
        let service = dir.join("systemd").join("trusttunnel.service");
        fs::write(&template, "[Service]\nExecStart=/bin/true\n").unwrap();
        fs::write(&service, "[Service]\nExecStart=/bin/true\n").unwrap();

        let changed = sync_service_template(&template, &service).unwrap();

        assert!(!changed);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_toml_string_assignment_extracts_value() {
        assert_eq!(
            parse_toml_string_assignment(r#"hostname = "vpn.example.com""#, "hostname"),
            Some("vpn.example.com".into())
        );
        assert_eq!(
            parse_toml_string_assignment(r#"username = "user1""#, "username"),
            Some("user1".into())
        );
    }

    #[test]
    fn infer_default_address_reads_first_hostname() {
        let dir = temp_test_dir("hosts");
        fs::create_dir_all(&dir).unwrap();
        let hosts = dir.join("hosts.toml");
        fs::write(
            &hosts,
            r#"
[[main_hosts]]
hostname = "vpn.example.com"

[[ping_hosts]]
hostname = "ping.example.com"
"#,
        )
        .unwrap();

        let address = infer_default_address(hosts.to_str().unwrap()).unwrap();

        assert_eq!(address.as_deref(), Some("vpn.example.com"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn infer_first_client_name_uses_credentials_file_from_settings() {
        let dir = temp_test_dir("credentials");
        fs::create_dir_all(dir.join("config")).unwrap();
        let settings = dir.join("config").join("vpn.toml");
        let credentials = dir.join("config").join("creds.toml");
        fs::write(&settings, r#"credentials_file = "creds.toml""#).unwrap();
        fs::write(
            &credentials,
            r#"
[[client]]
username = "demo-user"
password = "secret"
"#,
        )
        .unwrap();

        let client = infer_first_client_name(settings.to_str().unwrap()).unwrap();

        assert_eq!(client.as_deref(), Some("demo-user"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_credentials_path_falls_back_to_default_filename() {
        let dir = temp_test_dir("settings-default");
        fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("vpn.toml");
        fs::write(&settings, "listen_address = \"0.0.0.0:443\"").unwrap();

        let resolved = resolve_credentials_path(settings.to_str().unwrap()).unwrap();

        assert_eq!(resolved, dir.join("credentials.toml"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_deeplink_is_inlined_by_default() {
        assert!(should_inline_deeplink("deeplink"));
        assert!(!should_inline_deeplink("toml"));
    }
}
