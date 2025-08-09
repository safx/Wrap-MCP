use core::fmt;

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
    pub fn tool_name(&self) -> Option<&str> {
        match self {
            LogEntryContent::Request { tool_name, .. } => Some(tool_name),
            LogEntryContent::Response { tool_name, .. } => Some(tool_name),
            LogEntryContent::Error { tool_name, .. } => Some(tool_name),
            LogEntryContent::Stderr { .. } => None,
        }
    }

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

        // Keyword regex filtering
        if let Some(ref keyword) = filter.keyword {
            // Serialize content to string for searching
            let content_str = serde_json::to_string(&self.content).unwrap_or_default();
            if let Ok(re) = regex::Regex::new(keyword) {
                if !re.is_match(&content_str) {
                    return false;
                }
            } else if !content_str.contains(keyword) {
                // If regex is invalid, treat as literal string search
                return false;
            }
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

impl fmt::Display for LogEntryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogEntryType::Request => write!(f, "request"),
            LogEntryType::Response => write!(f, "response"),
            LogEntryType::Error => write!(f, "error"),
            LogEntryType::Stderr => write!(f, "stderr"),
        }
    }
}

impl From<&LogEntryContent> for LogEntryType {
    fn from(content: &LogEntryContent) -> Self {
        match content {
            LogEntryContent::Request { .. } => LogEntryType::Request,
            LogEntryContent::Response { .. } => LogEntryType::Response,
            LogEntryContent::Error { .. } => LogEntryType::Error,
            LogEntryContent::Stderr { .. } => LogEntryType::Stderr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn create_test_entry(
        id: usize,
        timestamp: DateTime<Utc>,
        content: LogEntryContent,
    ) -> LogEntry {
        LogEntry {
            id,
            timestamp,
            content,
        }
    }

    #[test]
    fn test_filter_by_tool_name() {
        let entry = create_test_entry(
            1,
            Utc::now(),
            LogEntryContent::Request {
                tool_name: "test_tool".to_string(),
                content: serde_json::json!({}),
            },
        );

        // Should match when tool_name matches
        let filter = LogFilter {
            tool_name: Some("test_tool".to_string()),
            entry_type: None,
            after: None,
            before: None,
            keyword: None,
        };
        assert!(entry.filter(&filter));

        // Should not match when tool_name differs
        let filter = LogFilter {
            tool_name: Some("other_tool".to_string()),
            entry_type: None,
            after: None,
            before: None,
            keyword: None,
        };
        assert!(!entry.filter(&filter));

        // Stderr should not match any tool_name
        let stderr_entry = create_test_entry(
            2,
            Utc::now(),
            LogEntryContent::Stderr {
                message: "error".to_string(),
            },
        );
        let filter = LogFilter {
            tool_name: Some("any_tool".to_string()),
            entry_type: None,
            after: None,
            before: None,
            keyword: None,
        };
        assert!(!stderr_entry.filter(&filter));
    }

    #[test]
    fn test_filter_by_entry_type() {
        let request_entry = create_test_entry(
            1,
            Utc::now(),
            LogEntryContent::Request {
                tool_name: "tool".to_string(),
                content: serde_json::json!({}),
            },
        );

        // Should match correct entry type
        let filter = LogFilter {
            tool_name: None,
            entry_type: Some("request".to_string()),
            after: None,
            before: None,
            keyword: None,
        };
        assert!(request_entry.filter(&filter));

        // Should not match wrong entry type
        let filter = LogFilter {
            tool_name: None,
            entry_type: Some("response".to_string()),
            after: None,
            before: None,
            keyword: None,
        };
        assert!(!request_entry.filter(&filter));
    }

    #[test]
    fn test_filter_by_timestamp() {
        let base_time = Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap();
        let entry = create_test_entry(
            1,
            base_time,
            LogEntryContent::Request {
                tool_name: "tool".to_string(),
                content: serde_json::json!({}),
            },
        );

        // Should match when timestamp is after "after" filter
        let filter = LogFilter {
            tool_name: None,
            entry_type: None,
            after: Some(Utc.with_ymd_and_hms(2024, 1, 15, 11, 0, 0).unwrap()),
            before: None,
            keyword: None,
        };
        assert!(entry.filter(&filter));

        // Should not match when timestamp is before or equal to "after" filter
        let filter = LogFilter {
            tool_name: None,
            entry_type: None,
            after: Some(Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap()),
            before: None,
            keyword: None,
        };
        assert!(!entry.filter(&filter));

        // Should match when timestamp is before "before" filter
        let filter = LogFilter {
            tool_name: None,
            entry_type: None,
            after: None,
            before: Some(Utc.with_ymd_and_hms(2024, 1, 15, 13, 0, 0).unwrap()),
            keyword: None,
        };
        assert!(entry.filter(&filter));

        // Should not match when timestamp is after or equal to "before" filter
        let filter = LogFilter {
            tool_name: None,
            entry_type: None,
            after: None,
            before: Some(Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap()),
            keyword: None,
        };
        assert!(!entry.filter(&filter));
    }

    #[test]
    fn test_filter_combined() {
        let entry = create_test_entry(
            1,
            Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap(),
            LogEntryContent::Response {
                tool_name: "my_tool".to_string(),
                request_id: 1,
                response: serde_json::json!({"result": "ok"}),
            },
        );

        // All conditions match
        let filter = LogFilter {
            tool_name: Some("my_tool".to_string()),
            entry_type: Some("response".to_string()),
            after: Some(Utc.with_ymd_and_hms(2024, 1, 15, 11, 0, 0).unwrap()),
            before: Some(Utc.with_ymd_and_hms(2024, 1, 15, 13, 0, 0).unwrap()),
            keyword: None,
        };
        assert!(entry.filter(&filter));

        // One condition doesn't match (tool_name)
        let filter = LogFilter {
            tool_name: Some("wrong_tool".to_string()),
            entry_type: Some("response".to_string()),
            after: Some(Utc.with_ymd_and_hms(2024, 1, 15, 11, 0, 0).unwrap()),
            before: Some(Utc.with_ymd_and_hms(2024, 1, 15, 13, 0, 0).unwrap()),
            keyword: None,
        };
        assert!(!entry.filter(&filter));
    }

    #[test]
    fn test_empty_filter() {
        let entry = create_test_entry(
            1,
            Utc::now(),
            LogEntryContent::Error {
                tool_name: "tool".to_string(),
                request_id: 1,
                error: "test error".to_string(),
            },
        );

        // Empty filter should match everything
        let filter = LogFilter {
            tool_name: None,
            entry_type: None,
            after: None,
            before: None,
            keyword: None,
        };
        assert!(entry.filter(&filter));
    }

    #[test]
    fn test_filter_by_keyword_regex() {
        let entry = create_test_entry(
            1,
            Utc::now(),
            LogEntryContent::Request {
                tool_name: "search_tool".to_string(),
                content: serde_json::json!({
                    "query": "find important document",
                    "options": {"case_sensitive": false}
                }),
            },
        );

        // Should match with regex pattern
        let filter = LogFilter {
            tool_name: None,
            entry_type: None,
            after: None,
            before: None,
            keyword: Some(r"important\s+doc".to_string()),
        };
        assert!(entry.filter(&filter));

        // Should not match when pattern doesn't exist
        let filter = LogFilter {
            tool_name: None,
            entry_type: None,
            after: None,
            before: None,
            keyword: Some(r"missing\s+pattern".to_string()),
        };
        assert!(!entry.filter(&filter));

        // Test case-insensitive regex
        let filter = LogFilter {
            tool_name: None,
            entry_type: None,
            after: None,
            before: None,
            keyword: Some(r"(?i)IMPORTANT".to_string()),
        };
        assert!(entry.filter(&filter));
    }

    #[test]
    fn test_filter_by_keyword_literal() {
        let entry = create_test_entry(
            1,
            Utc::now(),
            LogEntryContent::Error {
                tool_name: "database".to_string(),
                request_id: 1,
                error: "Connection timeout after 30 seconds".to_string(),
            },
        );

        // Invalid regex should fall back to literal string search
        let filter = LogFilter {
            tool_name: None,
            entry_type: None,
            after: None,
            before: None,
            keyword: Some("timeout[".to_string()), // Invalid regex (unclosed bracket)
        };
        // Should not match because "timeout[" is not in the content literally
        assert!(!entry.filter(&filter));

        // Valid literal search
        let filter = LogFilter {
            tool_name: None,
            entry_type: None,
            after: None,
            before: None,
            keyword: Some("timeout".to_string()),
        };
        assert!(entry.filter(&filter));
    }
}
