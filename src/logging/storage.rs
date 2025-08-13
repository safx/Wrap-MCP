use regex::Regex;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

use crate::config::LogConfig;
use crate::logging::{LogEntry, LogFilter};
use crate::types::{RequestId, ToolName};

// Compile the ANSI regex once at startup
static ANSI_REGEX: OnceLock<Regex> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct LogStorage {
    entries: Arc<RwLock<VecDeque<LogEntry>>>,
    next_id: Arc<RwLock<usize>>,
    max_entries: usize,
    ansi_removal_enabled: Arc<RwLock<bool>>,
}

impl LogStorage {
    pub fn new(config: &LogConfig) -> Self {
        Self::new_with_max_entries(config.log_size)
    }

    pub fn new_with_max_entries(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(VecDeque::new())),
            next_id: Arc::new(RwLock::new(1)),
            max_entries,
            ansi_removal_enabled: Arc::new(RwLock::new(true)),
        }
    }

    async fn get_next_id(&self) -> RequestId {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;
        RequestId::new(id)
    }

    async fn add_entry(&self, entry: LogEntry) {
        let mut entries = self.entries.write().await;
        entries.push_back(entry);
        self.trim_entries(&mut entries).await;
    }

    async fn trim_entries(&self, entries: &mut VecDeque<LogEntry>) {
        if entries.len() > self.max_entries {
            let remove_count = entries.len() - self.max_entries;
            entries.drain(..remove_count);
            tracing::debug!("Trimmed {remove_count} old log entries");
        }
    }

    pub async fn add_request(&self, tool_name: String, arguments: Value) -> RequestId {
        let id = self.get_next_id().await;
        let entry = LogEntry::new_request(id, ToolName::from(tool_name), arguments);
        self.add_entry(entry).await;
        tracing::info!("Logged request #{}", id);
        id
    }

    pub async fn add_response(&self, request_id: RequestId, tool_name: String, response: Value) {
        let id = self.get_next_id().await;
        tracing::info!("Logged response #{} for request #{}", id, request_id);
        let entry = LogEntry::new_response(id, ToolName::from(tool_name), request_id, response);
        self.add_entry(entry).await;
    }

    pub async fn add_error(&self, request_id: RequestId, tool_name: String, error_message: String) {
        let id = self.get_next_id().await;
        tracing::error!(
            "Logged error #{} for request #{}: {}",
            id,
            request_id,
            error_message
        );
        let entry = LogEntry::new_error(id, ToolName::from(tool_name), request_id, error_message);
        self.add_entry(entry).await;
    }

    pub async fn add_stderr(&self, message: String) {
        let id = self.get_next_id().await;
        tracing::warn!("Logged stderr #{}: {}", id, message);

        // Remove ANSI escape sequences if enabled
        let cleaned_message = if *self.ansi_removal_enabled.read().await {
            Self::remove_ansi_sequences(&message)
        } else {
            message
        };

        let entry = LogEntry::new_stderr(id, cleaned_message);
        self.add_entry(entry).await;
    }

    pub async fn get_logs(&self, limit: Option<usize>, filter: Option<LogFilter>) -> Vec<LogEntry> {
        let entries = self.entries.read().await;
        let mut result: Vec<LogEntry> = entries.iter().cloned().collect();

        if let Some(filter) = filter {
            result.retain(|entry| entry.filter(&filter));
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
        let regex = ANSI_REGEX.get_or_init(|| {
            Regex::new(r"\x1b\[[0-9;]*[mGKHJF]").expect("Failed to compile ANSI regex")
        });
        regex.replace_all(text, "").to_string()
    }
}
