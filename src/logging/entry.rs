use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LogEntryContent {
    Request {
        tool_name: String,
        content: Value,
    },
    Response {
        tool_name: String,
        request_id: usize,
        response: Value,
    },
    Error {
        tool_name: String,
        request_id: usize,
        error: String,
    },
    Stderr {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: usize,
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub content: LogEntryContent,
}

// Keep for backwards compatibility in filters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogEntryType {
    Request,
    Response,
    Error,
    Stderr,
}

impl From<LogEntryContent> for LogEntryType {
    fn from(content: LogEntryContent) -> Self {
        match content {
            LogEntryContent::Request { .. } => LogEntryType::Request,
            LogEntryContent::Response { .. } => LogEntryType::Response,
            LogEntryContent::Error { .. } => LogEntryType::Error,
            LogEntryContent::Stderr { .. } => LogEntryType::Stderr,
        }
    }
}
