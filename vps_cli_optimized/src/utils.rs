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
}

impl CliError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            suggestions: vec![],
            warnings: vec![],
            code: 1,
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

    pub fn output_and_exit(self, format: OutputFormat) -> ! {
        match format {
            OutputFormat::Json => {
                let env = OutputEnvelope::<serde_json::Value> {
                    ok: false,
                    data: None,
                    error: Some(self.message.clone()),
                    breadcrumbs: None,
                    warnings: self.warnings.clone(),
                    changed_files: vec![],
                    backups: vec![],
                    rolled_back: None,
                    sensitive: None,
                };
                println!("{}", serde_json::to_string(&env).unwrap());
            }
            _ => {
                eprintln!("错误: {}", self.message);
                for warning in &self.warnings {
                    eprintln!("警告: {}", warning);
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
