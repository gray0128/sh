use clap::{Arg, ArgAction, ArgMatches, Command};
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use tar::Archive;

use crate::utils::{Breadcrumb, CliError, OperationReport, OutputEnvelope, OutputFormat};

const REPO_OWNER: &str = "gray0128";
const REPO_NAME: &str = "sh";
const DEFAULT_LATEST_RELEASE: &str = "0.1.0";
const BINARY_NAME: &str = "vps-cli";

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

pub fn version_cli() -> Command {
    Command::new("version")
        .about("查看当前版本、平台信息和下载链接")
        .arg(
            Arg::new("remote")
                .long("remote")
                .action(ArgAction::SetTrue)
                .help("尝试查询 GitHub Release 的最新版本"),
        )
}

pub fn upgrade_cli() -> Command {
    Command::new("upgrade")
        .about("下载并升级到 GitHub Release 版本")
        .arg(
            Arg::new("version")
                .long("version")
                .value_name("VERSION")
                .help("指定目标版本，例如 0.1.0；默认尝试升级到最新 release"),
        )
        .arg(
            Arg::new("check")
                .long("check")
                .action(ArgAction::SetTrue)
                .help("仅检查是否有新版本，不执行下载与替换"),
        )
}

pub fn handle_version(matches: &ArgMatches, format: OutputFormat) -> Result<(), CliError> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let latest = if matches.get_flag("remote") {
        resolve_latest_release_version()?
    } else {
        DEFAULT_LATEST_RELEASE.to_string()
    };
    let mut report = OperationReport::default();
    let arch = release_arch_name().ok();
    let download_url = arch.as_ref().map(|arch| release_archive_url(&latest, arch));
    if arch.is_none() {
        report.warnings.push(
            "当前平台没有对应的 Linux release 包；如需升级，请在目标 Linux VPS 上执行。".into(),
        );
    }
    let data = json!({
        "current_version": current,
        "latest_release_version": latest,
        "binary": BINARY_NAME,
        "platform": std::env::consts::OS,
        "arch": arch,
        "download_url": download_url,
        "requires_root_hint": "大部分管理命令需要 root；自升级若目标路径不可写，也需要 sudo 或 root。"
    });
    emit_success(
        format,
        data,
        &report,
        Some(vec![crumb("检查升级", "vps-cli upgrade --check")]),
    );
    Ok(())
}

pub fn handle_upgrade(matches: &ArgMatches, format: OutputFormat) -> Result<(), CliError> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let desired = match matches.get_one::<String>("version").cloned() {
        Some(version) => normalize_release_version(&version)?,
        None => resolve_latest_release_version()?,
    };
    if !is_release_version(&current) {
        return Err(CliError::new(format!(
            "当前构建版本 {} 不是可识别的 release 版本，无法判断升级关系",
            current
        )));
    }
    let check_only = matches.get_flag("check");
    let cmp = compare_versions(&current, &desired)?;
    let arch = match release_arch_name() {
        Ok(arch) => arch,
        Err(err) if check_only => {
            let mut report = OperationReport::default();
            report.warnings.push(err.message.clone());
            emit_success(
                format,
                json!({
                    "current_version": current,
                    "target_version": desired,
                    "upgrade_available": false,
                    "supported": false,
                    "message": "当前平台没有对应的 Linux release 包"
                }),
                &report,
                None,
            );
            return Ok(());
        }
        Err(err) => return Err(err),
    };
    let archive_url = release_archive_url(&desired, &arch);
    let checksum_url = format!("{}.sha256", archive_url);

    if check_only {
        emit_success(
            format,
            json!({
                "current_version": current,
                "target_version": desired,
                "upgrade_available": cmp < 0,
                "download_url": archive_url,
                "checksum_url": checksum_url
            }),
            &OperationReport::default(),
            Some(vec![crumb("执行升级", "vps-cli upgrade")]),
        );
        return Ok(());
    }

    if cmp >= 0 {
        emit_success(
            format,
            json!({
                "current_version": current,
                "target_version": desired,
                "upgraded": false,
                "message": "当前已是最新 release 版本"
            }),
            &OperationReport::default(),
            None,
        );
        return Ok(());
    }

    let client = github_client()?;
    let archive_bytes = download_bytes(&client, &archive_url)?;
    verify_archive_checksum(&client, &archive_bytes, &checksum_url)?;
    let stage_dir = upgrade_stage_dir(&desired)?;
    let extracted = extract_release_archive(&archive_bytes, &stage_dir)?;
    let current_exe = env::current_exe()
        .map_err(|e| CliError::new(format!("获取当前可执行文件路径失败: {}", e)))?;
    let install_dir = current_exe
        .parent()
        .ok_or_else(|| CliError::new("无法识别当前可执行文件目录"))?;
    let temp_target = install_dir.join(format!("{}.upgrade.tmp", BINARY_NAME));
    copy_binary_for_replace(&extracted, &temp_target)?;
    fs::rename(&temp_target, &current_exe).map_err(|e| {
        let _ = fs::remove_file(&temp_target);
        permission_hint_error("替换当前可执行文件失败", e)
    })?;

    let mut report = OperationReport::default();
    report.changed_files.push(current_exe.display().to_string());
    report.rolled_back = Some(false);
    emit_success(
        format,
        json!({
            "upgraded": true,
            "from": current,
            "to": desired,
            "path": current_exe.display().to_string(),
            "download_url": archive_url
        }),
        &report,
        Some(vec![crumb("查看版本", "vps-cli version --remote")]),
    );
    Ok(())
}

fn github_client() -> Result<Client, CliError> {
    Client::builder()
        .user_agent(format!("vps-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| CliError::new(format!("初始化 HTTP 客户端失败: {}", e)))
}

fn resolve_latest_release_version() -> Result<String, CliError> {
    let client = github_client()?;
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        REPO_OWNER, REPO_NAME
    );
    let response = client.get(url).send();
    match response {
        Ok(resp) if resp.status().is_success() => {
            let body = resp
                .text()
                .map_err(|e| CliError::new(format!("读取最新 release 响应失败: {}", e)))?;
            let release: GitHubRelease = serde_json::from_str(&body)
                .map_err(|e| CliError::new(format!("解析最新 release 响应失败: {}", e)))?;
            normalize_release_version(&release.tag_name)
        }
        _ => Ok(DEFAULT_LATEST_RELEASE.to_string()),
    }
}

fn normalize_release_version(value: &str) -> Result<String, CliError> {
    let normalized = value.trim().trim_start_matches('v').to_string();
    if is_release_version(&normalized) {
        Ok(normalized)
    } else {
        Err(CliError::new(format!(
            "不是合法的 release 版本号：{}，应为 x.y.z",
            value
        )))
    }
}

fn is_release_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.parse::<u64>().is_ok())
}

fn compare_versions(left: &str, right: &str) -> Result<i32, CliError> {
    let parse = |value: &str| -> Result<Vec<u64>, CliError> {
        normalize_release_version(value)?
            .split('.')
            .map(|part| {
                part.parse::<u64>()
                    .map_err(|_| CliError::new(format!("无法解析版本号：{}", value)))
            })
            .collect()
    };
    let left = parse(left)?;
    let right = parse(right)?;
    for (l, r) in left.iter().zip(right.iter()) {
        if l < r {
            return Ok(-1);
        }
        if l > r {
            return Ok(1);
        }
    }
    Ok(0)
}

fn release_arch_name() -> Result<String, CliError> {
    if env::consts::OS != "linux" {
        return Err(CliError::new("当前仅为 Linux 提供 vps-cli release 安装包"));
    }
    match env::consts::ARCH {
        "x86_64" => Ok("amd64".into()),
        "aarch64" => Ok("arm64".into()),
        other => Err(CliError::new(format!(
            "当前架构暂不支持自动升级：{}",
            other
        ))),
    }
}

fn release_archive_url(version: &str, arch: &str) -> String {
    format!(
        "https://github.com/{}/{}/releases/download/v{}/vps-cli-linux-{}.tar.gz",
        REPO_OWNER, REPO_NAME, version, arch
    )
}

fn download_bytes(client: &Client, url: &str) -> Result<Vec<u8>, CliError> {
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| CliError::new(format!("下载失败 {}: {}", url, e)))?;
    if !response.status().is_success() {
        return Err(CliError::new(format!(
            "下载失败 {}: HTTP {}",
            url,
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .copy_to(&mut bytes)
        .map_err(|e| CliError::new(format!("读取下载内容失败: {}", e)))?;
    Ok(bytes)
}

fn download_text(client: &Client, url: &str) -> Result<String, CliError> {
    let response = client
        .get(url)
        .send()
        .map_err(|e| CliError::new(format!("下载失败 {}: {}", url, e)))?;
    if !response.status().is_success() {
        return Err(CliError::new(format!(
            "下载失败 {}: HTTP {}",
            url,
            response.status()
        )));
    }
    response
        .text()
        .map_err(|e| CliError::new(format!("读取文本内容失败: {}", e)))
}

fn verify_archive_checksum(
    client: &Client,
    archive: &[u8],
    checksum_url: &str,
) -> Result<(), CliError> {
    let expected = download_text(client, checksum_url)?;
    let expected = expected
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if expected.is_empty() {
        return Err(CliError::new("release SHA256 文件内容为空"));
    }
    let mut hasher = Sha256::new();
    hasher.update(archive);
    let actual = hex::encode(hasher.finalize());
    if expected != actual {
        return Err(CliError::new(format!(
            "release 包 SHA256 校验失败：期望 {}，实际 {}",
            expected, actual
        )));
    }
    Ok(())
}

fn upgrade_stage_dir(version: &str) -> Result<PathBuf, CliError> {
    let dir = env::temp_dir().join(format!("vps-cli-upgrade-{}", version));
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    fs::create_dir_all(&dir).map_err(|e| CliError::new(format!("创建升级临时目录失败: {}", e)))?;
    Ok(dir)
}

fn extract_release_archive(archive: &[u8], dir: &Path) -> Result<PathBuf, CliError> {
    let tar = GzDecoder::new(Cursor::new(archive));
    let mut archive = Archive::new(tar);
    let output = dir.join(BINARY_NAME);
    let mut found = false;
    let entries = archive
        .entries()
        .map_err(|e| CliError::new(format!("读取 release 压缩包失败: {}", e)))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|e| CliError::new(format!("解析 release 条目失败: {}", e)))?;
        let path = entry
            .path()
            .map_err(|e| CliError::new(format!("解析 release 文件路径失败: {}", e)))?;
        if path.file_name().and_then(|v| v.to_str()) == Some(BINARY_NAME) {
            entry
                .unpack(&output)
                .map_err(|e| CliError::new(format!("解压 vps-cli 失败: {}", e)))?;
            found = true;
            break;
        }
    }
    if !found {
        return Err(CliError::new("release 压缩包中未找到 vps-cli 可执行文件"));
    }
    set_exec_permissions(&output)?;
    Ok(output)
}

fn copy_binary_for_replace(source: &Path, target: &Path) -> Result<(), CliError> {
    let data = fs::read(source).map_err(|e| CliError::new(format!("读取升级二进制失败: {}", e)))?;
    let mut file =
        fs::File::create(target).map_err(|e| permission_hint_error("创建升级临时文件失败", e))?;
    file.write_all(&data)
        .map_err(|e| permission_hint_error("写入升级临时文件失败", e))?;
    set_exec_permissions(target)?;
    Ok(())
}

fn set_exec_permissions(path: &Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)
            .map_err(|e| CliError::new(format!("读取文件权限失败: {}", e)))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)
            .map_err(|e| CliError::new(format!("设置文件权限失败: {}", e)))?;
    }
    Ok(())
}

fn permission_hint_error(prefix: &str, error: io::Error) -> CliError {
    let mut err = CliError::new(format!("{}: {}", prefix, error));
    if matches!(error.kind(), io::ErrorKind::PermissionDenied) {
        err.suggestions.push(
            "当前可执行文件路径不可写；如果安装在 /usr/local/bin 等系统目录，请使用 sudo 或 root 运行升级。"
                .into(),
        );
    }
    err
}

fn emit_success(
    format: OutputFormat,
    data: serde_json::Value,
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
    use super::*;

    #[test]
    fn version_compare_works() {
        assert_eq!(compare_versions("0.1.0", "0.1.0").unwrap(), 0);
        assert_eq!(compare_versions("0.1.0", "0.2.0").unwrap(), -1);
        assert_eq!(compare_versions("1.2.0", "1.1.9").unwrap(), 1);
    }

    #[test]
    fn release_url_matches_arch() {
        assert_eq!(
            release_archive_url("0.1.0", "amd64"),
            "https://github.com/gray0128/sh/releases/download/v0.1.0/vps-cli-linux-amd64.tar.gz"
        );
    }

    #[test]
    fn release_version_validation_rejects_invalid_values() {
        assert!(normalize_release_version("v0.1.0").is_ok());
        assert!(normalize_release_version("latest").is_err());
    }
}
