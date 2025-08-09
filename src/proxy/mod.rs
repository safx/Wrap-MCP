use crate::logging::LogStorage;
use crate::wrappee::WrappeeClient;
use anyhow::Result;
use rmcp::{ErrorData as McpError, model::*};
use serde_json::Map;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct ProxyHandler {
    pub wrappee_tools: Arc<RwLock<Vec<Tool>>>,
    pub log_storage: Arc<LogStorage>,
}

impl ProxyHandler {
    pub fn new(log_storage: Arc<LogStorage>) -> Self {
        Self {
            wrappee_tools: Arc::new(RwLock::new(Vec::new())),
            log_storage,
        }
    }

    pub async fn discover_tools(&self, wrappee: &mut WrappeeClient) -> Result<()> {
        tracing::info!("Discovering tools from wrappee");

        let response = wrappee.list_tools().await?;

        if let Some(result) = response.get("result")
            && let Some(tools_value) = result.get("tools")
        {
            let tools: Vec<Tool> = serde_json::from_value(tools_value.clone())?;

            let mut wrappee_tools = self.wrappee_tools.write().await;
            *wrappee_tools = tools.clone();

            tracing::info!("Discovered {} tools from wrappee", tools.len());
            for tool in &tools {
                tracing::debug!(
                    "  - {}: {}",
                    tool.name,
                    tool.description.as_deref().unwrap_or("")
                );
            }
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
        let mut all_tools = wrappee_tools.clone();

        // Add show_log tool
        let show_log_schema: Map<String, Value> = serde_json::json!({
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
                "format": {
                    "type": "string",
                    "enum": ["ai", "text", "json"],
                    "description": "Output format (default: ai)",
                    "default": "ai"
                }
            }
        })
        .as_object()
        .unwrap()
        .clone();

        all_tools.push(Tool {
            name: "show_log".into(),
            description: Some("Display recorded request/response logs from the wrapper".into()),
            input_schema: Arc::new(show_log_schema),
            output_schema: None,
            annotations: None,
        });

        // Add clear_log tool
        let clear_log_schema: Map<String, Value> = serde_json::json!({
            "type": "object",
            "properties": {}
        })
        .as_object()
        .unwrap()
        .clone();

        all_tools.push(Tool {
            name: "clear_log".into(),
            description: Some("Clear all recorded logs".into()),
            input_schema: Arc::new(clear_log_schema),
            output_schema: None,
            annotations: None,
        });

        // Add restart_wrapped_server tool
        let restart_schema: Map<String, Value> = serde_json::json!({
            "type": "object",
            "properties": {}
        })
        .as_object()
        .unwrap()
        .clone();

        all_tools.push(Tool {
            name: "restart_wrapped_server".into(),
            description: Some("Restart the wrapped MCP server while preserving logs".into()),
            input_schema: Arc::new(restart_schema),
            output_schema: None,
            annotations: None,
        });

        all_tools
    }

    pub async fn proxy_tool_call(
        &self,
        name: &str,
        arguments: Value,
        wrappee: &mut WrappeeClient,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!("Proxying tool call: {}", name);

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
                        serde_json::from_value::<CallToolResult>(result.clone())
                    {
                        Ok(tool_result)
                    } else {
                        // Try to construct a CallToolResult from the response
                        Ok(CallToolResult::success(vec![Content::text(
                            serde_json::to_string_pretty(&result)
                                .unwrap_or_else(|_| result.to_string()),
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
                        serde_json::to_string_pretty(&response)
                            .unwrap_or_else(|_| response.to_string()),
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
