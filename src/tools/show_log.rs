use crate::logging::{LogEntry, LogEntryContent, LogEntryType, LogFilter, LogStorage};
use anyhow::Result;
use rmcp::{ErrorData as McpError, model::*};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

fn format_ai_output(logs: Vec<LogEntry>) -> Content {
    let mut output = String::new();

    if logs.is_empty() {
        output.push_str("No log entries found.\n");
    } else {
        for log in &logs {
            match &log.content {
                LogEntryContent::Request { tool_name, content } => {
                    let args = content;
                    let args_str = if let Some(obj) = args.as_object() {
                        obj.iter()
                            .map(|(k, v)| {
                                let v_str = match v {
                                    serde_json::Value::String(s) => format!("\"{s}\""),
                                    _ => v.to_string(),
                                };
                                format!("{k}: {v_str}")
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    } else {
                        args.to_string()
                    };
                    output.push_str(&format!(
                        "[REQUEST #{}] {}({})\n",
                        log.id, tool_name, args_str
                    ));
                }
                LogEntryContent::Response {
                    request_id,
                    response,
                    ..
                } => {
                    if let Some(result) = response.get("result")
                        && let Some(content_array) = result.get("content")
                            && let Some(arr) = content_array.as_array() {
                                for item in arr {
                                    if let Some(text) = item["text"].as_str() {
                                        output.push_str(&format!(
                                            "[RESPONSE #{request_id}] \"{text}\"\n"
                                        ));
                                    }
                                }
                            }
                }
                LogEntryContent::Error {
                    request_id, error, ..
                } => {
                    output.push_str(&format!("[ERROR #{request_id}] {error}\n"));
                }
                LogEntryContent::Stderr { message } => {
                    // Extract the essential part of stderr message
                    // Look for log prefix pattern like "2025-08-08T16:15:53.880856Z  INFO ThreadId(01) module::path: file.rs:123: "
                    let clean_msg = if let Some(start) = message.find(": src/") {
                        // Find the position after the file location
                        if let Some(pos) = message[start..].find(": ") {
                            &message[start + pos + 2..]
                        } else {
                            message
                        }
                    } else if message.contains(" INFO ")
                        || message.contains(" WARN ")
                        || message.contains(" ERROR ")
                        || message.contains(" DEBUG ")
                    {
                        // For other log messages, try to extract after the module path
                        if let Some(module_start) = message.rfind(" ThreadId") {
                            if let Some(msg_start) = message[module_start..].find(": ") {
                                if let Some(second_colon) =
                                    message[module_start + msg_start + 2..].find(": ")
                                {
                                    &message[module_start + msg_start + 2 + second_colon + 2..]
                                } else {
                                    &message[module_start + msg_start + 2..]
                                }
                            } else {
                                message
                            }
                        } else {
                            message
                        }
                    } else {
                        message
                    };
                    output.push_str(&format!("[STDERR] {clean_msg}\n"));
                }
            }
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
        "json" => Content::text(serde_json::to_string_pretty(&logs).unwrap()),
        "ai" => format_ai_output(logs),
        "text" => format_text_output(logs),
        _ => {
            // Fallback to AI format for any unrecognized format
            format_ai_output(logs)
        }
    };

    Ok(CallToolResult::success(vec![content]))
}
