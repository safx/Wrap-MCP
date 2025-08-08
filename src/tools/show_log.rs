use crate::logging::{LogFilter, LogStorage};
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

    let format = req.format.as_deref().unwrap_or("text");

    let content = match format {
        "json" => {
            let json_output = serde_json::json!({
                "total_count": total_count,
                "showing": logs.len(),
                "logs": logs
            });
            Content::text(serde_json::to_string_pretty(&json_output).unwrap())
        }
        "text" | _ => {
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
                            crate::logging::LogEntryType::Request => "📤 REQUEST",
                            crate::logging::LogEntryType::Response => "📥 RESPONSE",
                            crate::logging::LogEntryType::Error => "❌ ERROR",
                            crate::logging::LogEntryType::Stderr => "⚠️ STDERR",
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
