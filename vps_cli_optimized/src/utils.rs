use serde::{Serialize, Deserialize};
use std::{fmt, process};

#[derive(Debug, Clone, Copy)]
pub enum OutputFormat {
    Human,
    Plain,
    Json,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OutputEnvelope<T> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breadcrumbs: Option<Vec<Breadcrumb>>, // suggestions for next commands
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Breadcrumb {
    pub action: String,
    pub cmd: String,
}

#[derive(Debug)]
pub struct CliError {
    pub message: String,
    pub suggestions: Vec<String>,
    pub code: i32,
}

impl CliError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self { message: msg.into(), suggestions: vec![], code: 1 }
    }
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions = suggestions;
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
                };
                println!("{}", serde_json::to_string(&env).unwrap());
            }
            _ => {
                eprintln!("错误: {}", self.message);
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
