use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: usize,
    pub timestamp: DateTime<Utc>,
    pub entry_type: LogEntryType,
    pub tool_name: Option<String>,
    pub content: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogEntryType {
    Request,
    Response,
    Error,
    Stderr,
}

#[derive(Debug, Clone)]
pub struct LogStorage {
    entries: Arc<RwLock<Vec<LogEntry>>>,
    next_id: Arc<RwLock<usize>>,
    max_entries: usize,
    ansi_removal_enabled: Arc<RwLock<bool>>,
}

impl LogStorage {
    pub fn new() -> Self {
        let max_entries = env::var("WRAP_MCP_LOGSIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1000);

        tracing::info!("Log storage initialized with max entries: {}", max_entries);

        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
            next_id: Arc::new(RwLock::new(1)),
            max_entries,
            ansi_removal_enabled: Arc::new(RwLock::new(true)), // Default to removing ANSI
        }
    }

    async fn trim_entries(&self, entries: &mut Vec<LogEntry>) {
        if entries.len() > self.max_entries {
            let remove_count = entries.len() - self.max_entries;
            entries.drain(0..remove_count);
            tracing::debug!("Trimmed {} old log entries", remove_count);
        }
    }

    pub async fn add_request(&self, tool_name: Option<String>, content: Value) -> usize {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;

        let entry = LogEntry {
            id,
            timestamp: Utc::now(),
            entry_type: LogEntryType::Request,
            tool_name,
            content,
        };

        let mut entries = self.entries.write().await;
        entries.push(entry);
        self.trim_entries(&mut entries).await;

        tracing::info!("Logged request #{}", id);
        id
    }

    pub async fn add_response(&self, request_id: usize, tool_name: Option<String>, content: Value) {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;

        let entry = LogEntry {
            id,
            timestamp: Utc::now(),
            entry_type: LogEntryType::Response,
            tool_name,
            content: serde_json::json!({
                "request_id": request_id,
                "response": content
            }),
        };

        let mut entries = self.entries.write().await;
        entries.push(entry);
        self.trim_entries(&mut entries).await;

        tracing::info!("Logged response #{} for request #{}", id, request_id);
    }

    pub async fn add_error(&self, request_id: usize, tool_name: Option<String>, error: String) {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;

        let entry = LogEntry {
            id,
            timestamp: Utc::now(),
            entry_type: LogEntryType::Error,
            tool_name,
            content: serde_json::json!({
                "request_id": request_id,
                "error": error
            }),
        };

        let mut entries = self.entries.write().await;
        entries.push(entry);
        self.trim_entries(&mut entries).await;

        tracing::error!(
            "Logged error #{} for request #{}: {}",
            id,
            request_id,
            error
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
            entry_type: LogEntryType::Stderr,
            tool_name: None,
            content: serde_json::json!({
                "message": cleaned_message
            }),
        };

        let mut entries = self.entries.write().await;
        entries.push(entry);
        self.trim_entries(&mut entries).await;

        tracing::warn!("Logged stderr #{}: {}", id, message);
    }

    pub async fn get_logs(&self, limit: Option<usize>, filter: Option<LogFilter>) -> Vec<LogEntry> {
        let entries = self.entries.read().await;
        let mut result: Vec<LogEntry> = entries.clone();

        if let Some(filter) = filter {
            result.retain(|entry| {
                if let Some(ref tool_name) = filter.tool_name {
                    if entry.tool_name.as_ref() != Some(tool_name) {
                        return false;
                    }
                }
                if let Some(ref entry_type) = filter.entry_type {
                    if !matches!(
                        (&entry.entry_type, entry_type.as_str()),
                        (LogEntryType::Request, "request")
                            | (LogEntryType::Response, "response")
                            | (LogEntryType::Error, "error")
                            | (LogEntryType::Stderr, "stderr")
                    ) {
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

#[derive(Debug, Clone, Deserialize)]
pub struct LogFilter {
    pub tool_name: Option<String>,
    pub entry_type: Option<String>,
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
}

impl Default for LogStorage {
    fn default() -> Self {
        Self::new()
    }
}
