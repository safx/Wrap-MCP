use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::logging::LogFilter;

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

impl LogEntryContent {
    pub fn match_tool_name(&self, name: &str) -> bool {
        match self {
            LogEntryContent::Request { tool_name, .. } => tool_name == name,
            LogEntryContent::Response { tool_name, .. } => tool_name == name,
            LogEntryContent::Error { tool_name, .. } => tool_name == name,
            LogEntryContent::Stderr { .. } => false,
        }
    }

    pub fn match_entry_type(&self, entry_type: &str) -> bool {
        match self {
            LogEntryContent::Request { .. } => entry_type == "request",
            LogEntryContent::Response { .. } => entry_type == "response",
            LogEntryContent::Error { .. } => entry_type == "error",
            LogEntryContent::Stderr { .. } => entry_type == "stderr",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: usize,
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub content: LogEntryContent,
}

impl LogEntry {
    pub fn new_request(id: usize, tool_name: String, content: Value) -> Self {
        Self {
            id,
            timestamp: Utc::now(),
            content: LogEntryContent::Request { tool_name, content },
        }
    }
    pub fn new_response(id: usize, tool_name: String, request_id: usize, response: Value) -> Self {
        Self {
            id,
            timestamp: Utc::now(),
            content: LogEntryContent::Response {
                tool_name,
                request_id,
                response,
            },
        }
    }
    pub fn new_error(id: usize, tool_name: String, request_id: usize, error: String) -> Self {
        Self {
            id,
            timestamp: Utc::now(),
            content: LogEntryContent::Error {
                tool_name,
                request_id,
                error,
            },
        }
    }
    pub fn new_stderr(id: usize, message: String) -> Self {
        Self {
            id,
            timestamp: Utc::now(),
            content: LogEntryContent::Stderr { message },
        }
    }

    pub fn filter(&self, filter: &LogFilter) -> bool {
        if let Some(ref filter_tool_name) = filter.tool_name
            && !self.content.match_tool_name(filter_tool_name)
        {
            return false;
        }

        if let Some(ref entry_type) = filter.entry_type
            && !self.content.match_entry_type(entry_type)
        {
            return false;
        }
        if let Some(ref after) = filter.after
            && self.timestamp <= *after
        {
            return false;
        }
        if let Some(ref before) = filter.before
            && self.timestamp >= *before
        {
            return false;
        }
        true
    }
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
