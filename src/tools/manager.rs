use crate::logging::LogStorage;
use crate::wrappee::WrappeeClient;
use anyhow::Result;
use rmcp::{ErrorData as McpError, model::*};
use serde_json::Map;
use serde_json::Value;
use std::borrow::Cow;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct ToolManager {
    pub wrappee_tools: Arc<RwLock<Vec<Tool>>>,
    pub log_storage: Arc<LogStorage>,
}

impl ToolManager {
    pub fn new(log_storage: Arc<LogStorage>) -> Self {
        Self {
            wrappee_tools: Arc::new(RwLock::new(Vec::new())),
            log_storage,
        }
    }

    pub async fn discover_tools(&self, wrappee: &mut WrappeeClient) -> Result<()> {
        tracing::info!("Discovering tools from wrappee");

        let response = wrappee.list_tools().await?;
        tracing::info!(
            "tools/list response: {}",
            serde_json::to_string_pretty(&response)?
        );

        if let Some(result) = response.get("result")
            && let Some(tools_value) = result.get("tools")
        {
            let tools: Vec<Tool> = serde_json::from_value(tools_value.to_owned())?;

            let mut wrappee_tools = self.wrappee_tools.write().await;

            let len = tools.len();
            tracing::info!("Discovered {len} tools from wrappee");

            for tool in &tools {
                tracing::debug!(
                    "  - {}: {}",
                    tool.name,
                    tool.description.as_deref().unwrap_or("")
                );
            }
            *wrappee_tools = tools;
        }

        Ok(())
    }

    pub async fn clear_tools(&self) {
        let mut wrappee_tools = self.wrappee_tools.write().await;
        wrappee_tools.clear();
        tracing::info!("Cleared all discovered tools");
    }

    pub async fn get_all_tools(&self) -> Vec<Tool> {
        let wrappee_tools = self.wrappee_tools.read().await;

        // Create a new vector with capacity for all tools
        let mut all_tools = Vec::with_capacity(wrappee_tools.len() + 3);
        all_tools.extend(wrappee_tools.iter().cloned());

        // Add wrapper-provided tools
        all_tools.push(create_show_log_tool());
        all_tools.push(create_clear_log_tool());
        all_tools.push(create_restart_wrapped_server_tool());

        all_tools
    }

    pub async fn proxy_tool_call(
        &self,
        name: &str,
        arguments: Value,
        wrappee: &mut WrappeeClient,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!("Proxying tool call: {name}");

        // Log the request
        let request_id = self
            .log_storage
            .add_request(name.to_string(), arguments.clone())
            .await;

        // Forward to wrappee
        match wrappee.call_tool(name, arguments).await {
            Ok(response) => {
                // Log the response
                self.log_storage
                    .add_response(request_id, name.to_string(), response.clone())
                    .await;

                // Extract the result from the response
                if let Some(result) = response.get("result") {
                    if let Ok(tool_result) =
                        serde_json::from_value::<CallToolResult>(result.to_owned())
                    {
                        Ok(tool_result)
                    } else {
                        // Try to construct a CallToolResult from the response
                        Ok(CallToolResult::success(vec![Content::text(
                            serde_json::to_string(&result).unwrap_or_else(|_| result.to_string()),
                        )]))
                    }
                } else if let Some(error) = response.get("error") {
                    let error_msg = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown error")
                        .to_string();

                    let error_data = error.get("data").cloned();

                    self.log_storage
                        .add_error(request_id, name.to_string(), error_msg.clone())
                        .await;

                    Err(McpError {
                        code: ErrorCode::INTERNAL_ERROR,
                        message: error_msg.into(),
                        data: error_data,
                    })
                } else {
                    Ok(CallToolResult::success(vec![Content::text(
                        serde_json::to_string(&response).unwrap_or_else(|_| response.to_string()),
                    )]))
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to call tool: {e}");

                self.log_storage
                    .add_error(request_id, name.to_string(), error_msg.clone())
                    .await;

                Err(McpError {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: error_msg.into(),
                    data: None,
                })
            }
        }
    }
}

// Tool creation functions

fn create_show_log_tool() -> Tool {
    create_tool(
        Cow::Borrowed("show_log"),
        Cow::Borrowed("Display recorded request/response logs from the wrapper"),
        serde_json::json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of log entries to show (default: 20)",
                    "default": 20
                },
                "tool_name": {
                    "type": "string",
                    "description": "Filter logs by tool name"
                },
                "entry_type": {
                    "type": "string",
                    "enum": ["request", "response", "error", "stderr"],
                    "description": "Filter logs by entry type"
                },
                "keyword": {
                    "type": "string",
                    "description": "Regular expression pattern to search in log content (fallback to literal search if invalid regex)"
                },
                "format": {
                    "type": "string",
                    "enum": ["ai", "text", "json"],
                    "description": "Output format (default: ai)",
                    "default": "ai"
                }
            }
        }),
    )
}

fn create_clear_log_tool() -> Tool {
    create_tool(
        Cow::Borrowed("clear_log"),
        Cow::Borrowed("Clear all recorded logs"),
        serde_json::json!({}),
    )
}

fn create_restart_wrapped_server_tool() -> Tool {
    create_tool(
        Cow::Borrowed("restart_wrapped_server"),
        Cow::Borrowed("Restart the wrapped MCP server while preserving logs"),
        serde_json::json!({}),
    )
}

fn create_tool(name: Cow<'static, str>, description: Cow<'static, str>, properties: Value) -> Tool {
    let mut schema = Map::new();
    schema.insert("type".into(), "object".into());
    schema.insert("properties".into(), properties);

    Tool {
        name,
        description: Some(description),
        input_schema: Arc::new(schema),
        output_schema: None,
        annotations: None,
    }
}
