use serde::{Deserialize, Serialize};
use std::{fmt, process};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Plain,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breadcrumb {
    pub action: String,
    pub cmd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OperationReport {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub changed_files: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub backups: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rolled_back: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensitive: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OutputEnvelope<T> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breadcrumbs: Option<Vec<Breadcrumb>>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub changed_files: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub backups: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rolled_back: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensitive: Option<bool>,
}

impl<T> OutputEnvelope<T> {
    pub fn success(data: T) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
            breadcrumbs: None,
            warnings: vec![],
            changed_files: vec![],
            backups: vec![],
            rolled_back: None,
            sensitive: None,
        }
    }

    pub fn with_breadcrumbs(mut self, breadcrumbs: Vec<Breadcrumb>) -> Self {
        self.breadcrumbs = Some(breadcrumbs);
        self
    }

    pub fn with_report(mut self, report: &OperationReport) -> Self {
        self.warnings = report.warnings.clone();
        self.changed_files = report.changed_files.clone();
        self.backups = report.backups.clone();
        self.rolled_back = report.rolled_back;
        self.sensitive = report.sensitive;
        self
    }
}

#[derive(Debug)]
pub struct CliError {
    pub message: String,
    pub suggestions: Vec<String>,
    pub warnings: Vec<String>,
    pub code: i32,
    pub report: OperationReport,
}

impl CliError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            suggestions: vec![],
            warnings: vec![],
            code: 1,
            report: OperationReport::default(),
        }
    }

    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }

    pub fn with_code(mut self, code: i32) -> Self {
        self.code = code;
        self
    }

    pub fn with_report(mut self, report: &OperationReport) -> Self {
        self.report = report.clone();
        self
    }

    fn merged_warnings(&self) -> Vec<String> {
        let mut warnings = self.report.warnings.clone();
        for warning in &self.warnings {
            if !warnings.contains(warning) {
                warnings.push(warning.clone());
            }
        }
        warnings
    }

    fn json_envelope(&self) -> OutputEnvelope<serde_json::Value> {
        OutputEnvelope::<serde_json::Value> {
            ok: false,
            data: None,
            error: Some(self.message.clone()),
            breadcrumbs: None,
            warnings: self.merged_warnings(),
            changed_files: self.report.changed_files.clone(),
            backups: self.report.backups.clone(),
            rolled_back: self.report.rolled_back,
            sensitive: self.report.sensitive,
        }
    }

    pub fn output_and_exit(self, format: OutputFormat) -> ! {
        match format {
            OutputFormat::Json => {
                let env = self.json_envelope();
                println!("{}", serde_json::to_string(&env).unwrap());
            }
            _ => {
                eprintln!("错误: {}", self.message);
                for warning in self.merged_warnings() {
                    eprintln!("警告: {}", warning);
                }
                if !self.report.changed_files.is_empty() {
                    eprintln!("变更文件:");
                    for item in &self.report.changed_files {
                        eprintln!("- {}", item);
                    }
                }
                if !self.report.backups.is_empty() {
                    eprintln!("备份:");
                    for item in &self.report.backups {
                        eprintln!("- {}", item);
                    }
                }
                if let Some(rolled_back) = self.report.rolled_back {
                    eprintln!("已自动回滚: {}", if rolled_back { "是" } else { "否" });
                }
                if !self.suggestions.is_empty() {
                    eprintln!("建议: {}", self.suggestions.join("; "));
                }
            }
        }
        process::exit(self.code);
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CliError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_error_json_includes_operation_report() {
        let mut report = OperationReport::default();
        report.warnings.push("已尝试自动回滚".into());
        report.changed_files.push("/etc/example.conf".into());
        report.backups.push("/root/example.bak".into());
        report.rolled_back = Some(true);
        report.sensitive = Some(false);

        let env = CliError::new("操作失败")
            .with_warnings(vec!["请检查服务状态".into()])
            .with_report(&report)
            .json_envelope();

        assert!(!env.ok);
        assert_eq!(env.error.as_deref(), Some("操作失败"));
        assert!(env.warnings.contains(&"已尝试自动回滚".to_string()));
        assert!(env.warnings.contains(&"请检查服务状态".to_string()));
        assert_eq!(env.changed_files, vec!["/etc/example.conf"]);
        assert_eq!(env.backups, vec!["/root/example.bak"]);
        assert_eq!(env.rolled_back, Some(true));
        assert_eq!(env.sensitive, Some(false));
    }
}
