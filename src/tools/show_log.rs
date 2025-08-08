use crate::logging::{LogEntry, LogEntryType, LogFilter, LogStorage};
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
            match log.entry_type {
                LogEntryType::Request => {
                    if let Some(tool_name) = &log.tool_name {
                        let args = &log.content["arguments"];
                        let args_str = if let Some(obj) = args.as_object() {
                            obj.iter()
                                .map(|(k, v)| {
                                    let v_str = match v {
                                        serde_json::Value::String(s) => format!("\"{}\"", s),
                                        _ => v.to_string()
                                    };
                                    format!("{}: {}", k, v_str)
                                })
                                .collect::<Vec<_>>()
                                .join(", ")
                        } else {
                            args.to_string()
                        };
                        output.push_str(&format!("[REQUEST #{}] {}({})\n", log.id, tool_name, args_str));
                    }
                }
                LogEntryType::Response => {
                    if let Some(request_id) = log.content["request_id"].as_u64() {
                        if let Some(response) = log.content.get("response") {
                            if let Some(result) = response.get("result") {
                                if let Some(content_array) = result.get("content") {
                                    if let Some(arr) = content_array.as_array() {
                                        for item in arr {
                                            if let Some(text) = item["text"].as_str() {
                                                output.push_str(&format!("[RESPONSE #{}] \"{}\"\n", request_id, text));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                LogEntryType::Error => {
                    if let Some(request_id) = log.content["request_id"].as_u64() {
                        if let Some(error) = log.content["error"].as_str() {
                            output.push_str(&format!("[ERROR #{}] {}\n", request_id, error));
                        }
                    }
                }
                LogEntryType::Stderr => {
                    if let Some(msg) = log.content["message"].as_str() {
                        // Extract the essential part of stderr message
                        // Look for log prefix pattern like "2025-08-08T16:15:53.880856Z  INFO ThreadId(01) module::path: file.rs:123: "
                        let clean_msg = if let Some(start) = msg.find(": src/") {
                            // Find the position after the file location
                            if let Some(pos) = msg[start..].find(": ") {
                                &msg[start + pos + 2..]
                            } else {
                                msg
                            }
                        } else if msg.contains(" INFO ") || msg.contains(" WARN ") || msg.contains(" ERROR ") || msg.contains(" DEBUG ") {
                            // For other log messages, try to extract after the module path
                            if let Some(module_start) = msg.rfind(" ThreadId") {
                                if let Some(msg_start) = msg[module_start..].find(": ") {
                                    if let Some(second_colon) = msg[module_start + msg_start + 2..].find(": ") {
                                        &msg[module_start + msg_start + 2 + second_colon + 2..]
                                    } else {
                                        &msg[module_start + msg_start + 2..]
                                    }
                                } else {
                                    msg
                                }
                            } else {
                                msg
                            }
                        } else {
                            msg
                        };
                        output.push_str(&format!("[STDERR] {}\n", clean_msg));
                    }
                }
            }
            output.push('\n');
        }
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
    };

    let logs = log_storage.get_logs(Some(req.limit), Some(filter)).await;
    let total_count = log_storage.get_log_count().await;

    let format = req.format.as_deref().unwrap_or("ai");  // Default to AI format

    let content = match format {
        "json" => {
            let json_output = serde_json::json!({
                "total_count": total_count,
                "showing": logs.len(),
                "logs": logs
            });
            Content::text(serde_json::to_string_pretty(&json_output).unwrap())
        }
        "ai" => {
            format_ai_output(logs)
        }
        "text" => {
            let mut output = format!(
                "📊 Log Entries (showing {} of {})\n",
                logs.len(),
                total_count
            );
            output.push_str("═".repeat(60).as_str());
            output.push('\n');

            if logs.is_empty() {
                output.push_str("No log entries found.\n");
            } else {
                for log in logs {
                    output.push_str(&format!(
                        "\n[#{}] {} | {}\n",
                        log.id,
                        log.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
                        match log.entry_type {
                            LogEntryType::Request => "📤 REQUEST",
                            LogEntryType::Response => "📥 RESPONSE",
                            LogEntryType::Error => "❌ ERROR",
                            LogEntryType::Stderr => "⚠️ STDERR",
                        }
                    ));

                    if let Some(ref tool_name) = log.tool_name {
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
            }

            Content::text(output)
        }
        _ => {
            // Fallback to AI format for any unrecognized format
            format_ai_output(logs)
        }
    };

    Ok(CallToolResult::success(vec![content]))
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ClearLogRequest {}

pub async fn clear_log(
    _req: ClearLogRequest,
    log_storage: &LogStorage,
) -> Result<CallToolResult, McpError> {
    tracing::debug!("clear_log called");

    let count = log_storage.get_log_count().await;
    log_storage.clear_logs().await;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "✅ Cleared {count} log entries"
    ))]))
}
