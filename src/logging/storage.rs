use chrono::Utc;
use serde_json::Value;
use std::collections::VecDeque;
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::logging::{LogEntry, LogEntryContent, LogEntryType, LogFilter};

#[derive(Debug, Clone)]
pub struct LogStorage {
    entries: Arc<RwLock<VecDeque<LogEntry>>>,
    next_id: Arc<RwLock<usize>>,
    max_entries: usize,
    ansi_removal_enabled: Arc<RwLock<bool>>,
}

const DEFAULT_MAX_ENTRIES: usize = 1000;

impl LogStorage {
    pub fn new() -> Self {
        let max_entries = env::var("WRAP_MCP_LOGSIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MAX_ENTRIES);

        tracing::info!("Log storage initialized with max entries: {}", max_entries);

        Self {
            entries: Arc::new(RwLock::new(VecDeque::new())),
            next_id: Arc::new(RwLock::new(1)),
            max_entries,
            ansi_removal_enabled: Arc::new(RwLock::new(true)), // Default to removing ANSI
        }
    }

    async fn trim_entries(&self, entries: &mut VecDeque<LogEntry>) {
        if entries.len() > self.max_entries {
            let remove_count = entries.len() - self.max_entries;
            entries.drain(..remove_count);
            tracing::debug!("Trimmed {} old log entries", remove_count);
        }
    }

    pub async fn add_request(&self, tool_name: String, arguments: Value) -> usize {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;

        let entry = LogEntry {
            id,
            timestamp: Utc::now(),
            content: LogEntryContent::Request {
                tool_name: tool_name.clone(),
                content: serde_json::json!({
                    "tool": tool_name,
                    "arguments": arguments
                }),
            },
        };

        let mut entries = self.entries.write().await;
        entries.push_back(entry);
        self.trim_entries(&mut entries).await;

        tracing::info!("Logged request #{}", id);
        id
    }

    pub async fn add_response(&self, request_id: usize, tool_name: String, response: Value) {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;

        let entry = LogEntry {
            id,
            timestamp: Utc::now(),
            content: LogEntryContent::Response {
                tool_name,
                request_id,
                response,
            },
        };

        let mut entries = self.entries.write().await;
        entries.push_back(entry);
        self.trim_entries(&mut entries).await;

        tracing::info!("Logged response #{} for request #{}", id, request_id);
    }

    pub async fn add_error(&self, request_id: usize, tool_name: String, error_message: String) {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;

        let entry = LogEntry {
            id,
            timestamp: Utc::now(),
            content: LogEntryContent::Error {
                tool_name,
                request_id,
                error: error_message.clone(),
            },
        };

        let mut entries = self.entries.write().await;
        entries.push_back(entry);
        self.trim_entries(&mut entries).await;

        tracing::error!(
            "Logged error #{} for request #{}: {}",
            id,
            request_id,
            error_message
        );
    }

    pub async fn add_stderr(&self, message: String) {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;

        // Remove ANSI escape sequences if enabled
        let cleaned_message = if *self.ansi_removal_enabled.read().await {
            Self::remove_ansi_sequences(&message)
        } else {
            message.clone()
        };

        let entry = LogEntry {
            id,
            timestamp: Utc::now(),
            content: LogEntryContent::Stderr {
                message: cleaned_message,
            },
        };

        let mut entries = self.entries.write().await;
        entries.push_back(entry);
        self.trim_entries(&mut entries).await;

        tracing::warn!("Logged stderr #{}: {}", id, message);
    }

    pub async fn get_logs(&self, limit: Option<usize>, filter: Option<LogFilter>) -> Vec<LogEntry> {
        let entries = self.entries.read().await;
        let mut result: Vec<LogEntry> = entries.iter().cloned().collect();

        if let Some(filter) = filter {
            result.retain(|entry| {
                if let Some(ref filter_tool_name) = filter.tool_name {
                    let matches = match &entry.content {
                        LogEntryContent::Request { tool_name, .. }
                        | LogEntryContent::Response { tool_name, .. }
                        | LogEntryContent::Error { tool_name, .. } => tool_name == filter_tool_name,
                        LogEntryContent::Stderr { .. } => false,
                    };
                    if !matches {
                        return false;
                    }
                }
                if let Some(ref entry_type) = filter.entry_type {
                    let entry_log_type: LogEntryType = entry.content.clone().into();
                    let matches = match (entry_log_type, entry_type.as_str()) {
                        (LogEntryType::Request, "request")
                        | (LogEntryType::Response, "response")
                        | (LogEntryType::Error, "error")
                        | (LogEntryType::Stderr, "stderr") => true,
                        _ => false,
                    };
                    if !matches {
                        return false;
                    }
                }
                if let Some(ref after) = filter.after {
                    if entry.timestamp <= *after {
                        return false;
                    }
                }
                if let Some(ref before) = filter.before {
                    if entry.timestamp >= *before {
                        return false;
                    }
                }
                true
            });
        }

        // Sort by timestamp descending (newest first)
        result.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        if let Some(limit) = limit {
            result.truncate(limit);
        }

        result
    }

    pub async fn clear_logs(&self) {
        let mut entries = self.entries.write().await;
        entries.clear();

        let mut next_id = self.next_id.write().await;
        *next_id = 1;

        tracing::info!("Cleared all logs");
    }

    pub async fn get_log_count(&self) -> usize {
        let entries = self.entries.read().await;
        entries.len()
    }

    pub async fn set_ansi_removal(&self, enabled: bool) {
        let mut ansi_removal = self.ansi_removal_enabled.write().await;
        *ansi_removal = enabled;
    }

    /// Remove ANSI escape sequences from a string
    fn remove_ansi_sequences(text: &str) -> String {
        // Pattern to match ANSI escape sequences
        // Matches: ESC[...m, ESC[...K, ESC[...H, ESC[...J, etc.
        let ansi_regex = regex::Regex::new(r"\x1b\[[0-9;]*[mGKHJF]").unwrap();
        ansi_regex.replace_all(text, "").to_string()
    }
}

impl Default for LogStorage {
    fn default() -> Self {
        Self::new()
    }
}
