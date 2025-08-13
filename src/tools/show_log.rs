use crate::logging::{LogEntry, LogEntryContent, LogEntryType, LogFilter, LogStorage};
use crate::types::RequestId;
use anyhow::Result;
use rmcp::{ErrorData as McpError, model::*};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ShowLogRequest {
    #[serde(default = "default_limit")]
    pub limit: usize,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

fn default_limit() -> usize {
    20
}

// Helper function to format request arguments
fn format_request_args(args: &Value) -> String {
    if let Some(obj) = args.as_object() {
        obj.iter()
            .map(|(k, v)| {
                let v_str = match v {
                    Value::String(s) => format!("\"{s}\""),
                    _ => v.to_string(),
                };
                format!("{k}: {v_str}")
            })
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        args.to_string()
    }
}

// Format a request log entry
fn format_request_entry(id: RequestId, tool_name: &str, args: &Value) -> String {
    let args_str = format_request_args(args);
    format!("[REQUEST #{id}] {tool_name}({args_str})\n")
}

// Format a response log entry
fn format_response_entry(request_id: RequestId, response: &Value) -> String {
    let mut output = String::new();

    if let Some(result) = response.get("result")
        && let Some(content_array) = result.get("content")
        && let Some(arr) = content_array.as_array()
    {
        for item in arr {
            if let Some(text) = item["text"].as_str() {
                output.push_str(&format!("[RESPONSE #{request_id}] \"{text}\"\n"));
            }
        }
    }

    output
}

// Format an error log entry
fn format_error_entry(request_id: RequestId, error: &str) -> String {
    format!("[ERROR #{request_id}] {error}\n")
}

// Clean and format stderr message
fn clean_stderr_message(message: &str) -> &str {
    // Look for log prefix pattern like "2025-08-08T16:15:53.880856Z  INFO ThreadId(01) module::path: file.rs:123: "
    if let Some(start) = message.find(": src/") {
        // Find the position after the file location
        if let Some(pos) = message[start..].find(": ") {
            return &message[start + pos + 2..];
        }
    }

    // For other log messages with level indicators
    if message.contains(" INFO ")
        || message.contains(" WARN ")
        || message.contains(" ERROR ")
        || message.contains(" DEBUG ")
    {
        // Try to extract after the module path
        if let Some(module_start) = message.rfind(" ThreadId")
            && let Some(msg_start) = message[module_start..].find(": ")
        {
            if let Some(second_colon) = message[module_start + msg_start + 2..].find(": ") {
                return &message[module_start + msg_start + 2 + second_colon + 2..];
            } else {
                return &message[module_start + msg_start + 2..];
            }
        }
    }

    message
}

// Format a stderr log entry
fn format_stderr_entry(message: &str) -> String {
    let clean_msg = clean_stderr_message(message);
    format!("[STDERR] {clean_msg}\n")
}

fn format_ai_output(logs: Vec<LogEntry>) -> Content {
    let mut output = String::new();

    if logs.is_empty() {
        output.push_str("No log entries found.\n");
    } else {
        for log in &logs {
            let formatted_entry = match &log.content {
                LogEntryContent::Request { tool_name, content } => {
                    format_request_entry(log.id, tool_name.as_str(), content)
                }
                LogEntryContent::Response {
                    request_id,
                    response,
                    ..
                } => format_response_entry(*request_id, response),
                LogEntryContent::Error {
                    request_id, error, ..
                } => format_error_entry(*request_id, error),
                LogEntryContent::Stderr { message } => format_stderr_entry(message),
            };

            output.push_str(&formatted_entry);
            output.push('\n');
        }
    }

    Content::text(output)
}

fn format_text_output(logs: Vec<LogEntry>) -> Content {
    let mut output = String::new();

    for log in logs {
        let content = &log.content;
        let entry_type: LogEntryType = content.into();
        output.push_str(&format!(
            "[#{}] {} | {}\n",
            log.id,
            log.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
            entry_type,
        ));

        if let Some(tool_name) = content.tool_name() {
            output.push_str(&format!("Tool: {tool_name}\n"));
        }

        output.push_str(&format!(
            "Content: {}\n",
            serde_json::to_string_pretty(&log.content)
                .unwrap_or_else(|_| "Failed to serialize".to_string())
        ));
        output.push_str("-".repeat(60).as_str());
        output.push('\n');
    }

    Content::text(output)
}

pub async fn show_log(
    req: ShowLogRequest,
    log_storage: &LogStorage,
) -> Result<CallToolResult, McpError> {
    tracing::debug!(
        "show_log called with limit: {}, tool_name: {:?}, entry_type: {:?}",
        req.limit,
        req.tool_name,
        req.entry_type
    );

    let filter = LogFilter {
        tool_name: req.tool_name,
        entry_type: req.entry_type,
        after: None,
        before: None,
        keyword: req.keyword,
    };

    let logs = log_storage.get_logs(Some(req.limit), Some(filter)).await;

    let format = req.format.as_deref().unwrap_or("ai").trim(); // Default to AI format
    let content = match format {
        "json" => Content::text(
            serde_json::to_string_pretty(&logs)
                .unwrap_or_else(|e| format!("Failed to serialize logs: {}", e)),
        ),
        "ai" => format_ai_output(logs),
        "text" => format_text_output(logs),
        _ => {
            // Fallback to AI format for any unrecognized format
            format_ai_output(logs)
        }
    };

    Ok(CallToolResult::success(vec![content]))
}
