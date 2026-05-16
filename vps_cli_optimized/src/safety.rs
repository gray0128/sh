use crate::utils::{CliError, OperationReport};
use dialoguer::Confirm;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as SysCmd;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn is_interactive(no_input: bool) -> bool {
    atty::is(atty::Stream::Stdin) && !no_input
}

pub fn require_confirmation(
    prompt: &str,
    interactive: bool,
    dry_run: bool,
    confirmed: bool,
) -> Result<(), CliError> {
    if dry_run || confirmed {
        return Ok(());
    }
    if interactive {
        let proceed = Confirm::new()
            .with_prompt(prompt)
            .default(false)
            .interact()
            .map_err(|e| CliError::new(format!("读取确认失败: {}", e)))?;
        if proceed {
            Ok(())
        } else {
            Err(CliError::new("已取消执行").with_code(0))
        }
    } else {
        Err(CliError::new("非交互模式下必须显式传入 --confirm"))
    }
}

pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn ensure_dir(path: &Path) -> Result<(), CliError> {
    fs::create_dir_all(path)
        .map_err(|e| CliError::new(format!("创建目录失败 {}: {}", path.display(), e)))
}

pub fn backup_path(
    path: &Path,
    backup_root: &Path,
    prefix: &str,
) -> Result<Option<PathBuf>, CliError> {
    if !path.exists() {
        return Ok(None);
    }
    ensure_dir(backup_root)?;
    let backup = backup_root.join(format!("{}.{}.bak", prefix, current_timestamp()));
    let status = SysCmd::new("cp")
        .args([
            "-a",
            path.to_string_lossy().as_ref(),
            backup.to_string_lossy().as_ref(),
        ])
        .status()
        .map_err(|e| CliError::new(format!("执行备份命令失败: {}", e)))?;
    if !status.success() {
        return Err(CliError::new(format!("备份失败: {}", path.display())));
    }
    Ok(Some(backup))
}

pub fn restore_backup(backup: &Path, target: &Path) -> Result<(), CliError> {
    let status = SysCmd::new("cp")
        .args([
            "-a",
            backup.to_string_lossy().as_ref(),
            target.to_string_lossy().as_ref(),
        ])
        .status()
        .map_err(|e| CliError::new(format!("执行恢复命令失败: {}", e)))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::new(format!(
            "恢复备份失败: {} -> {}",
            backup.display(),
            target.display()
        )))
    }
}

pub fn append_backup(report: &mut OperationReport, backup: &Option<PathBuf>) {
    if let Some(path) = backup {
        report.backups.push(path.display().to_string());
    }
}
