pub mod show_log;

use crate::{logging::LogStorage, proxy::ProxyHandler, wrappee::WrappeeClient};
use anyhow::Result;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, model::*, service::RequestContext};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct WrapServer {
    proxy_handler: Arc<ProxyHandler>,
    wrappee: Arc<RwLock<Option<WrappeeClient>>>,
}

impl Default for WrapServer {
    fn default() -> Self {
        Self::new()
    }
}

impl WrapServer {
    pub fn new() -> Self {
        let log_storage = Arc::new(LogStorage::new());
        let proxy_handler = Arc::new(ProxyHandler::new(log_storage));

        Self {
            proxy_handler,
            wrappee: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn initialize_wrappee(&self) -> Result<()> {
        // Parse command line arguments
        let args: Vec<String> = std::env::args().collect();

        // Find the "--" separator
        let separator_pos = args.iter().position(|arg| arg == "--");

        let (command, wrappee_args) = match separator_pos {
            Some(pos) if pos + 1 < args.len() => {
                // Get the command and arguments after "--"
                let command = args[pos + 1].clone();
                let wrappee_args = if pos + 2 < args.len() {
                    args[pos + 2..].to_vec()
                } else {
                    vec![]
                };
                (command, wrappee_args)
            }
            _ => {
                // No "--" found or no command after it, use default
                tracing::warn!(
                    "No wrappee command specified. Usage: wrap-mcp [options] -- <command> [args...]"
                );
                (
                    "echo".to_string(),
                    vec!["No wrappee command specified".to_string()],
                )
            }
        };

        tracing::info!(
            "Initializing wrappee with command: {} {:?}",
            command,
            wrappee_args
        );

        let mut wrappee_client = WrappeeClient::spawn(&command, &wrappee_args)?;

        // Initialize the wrappee
        wrappee_client.initialize().await?;

        // Discover tools from wrappee
        self.proxy_handler
            .discover_tools(&mut wrappee_client)
            .await?;

        // Store the wrappee client
        *self.wrappee.write().await = Some(wrappee_client);

        // Start stderr monitoring in the background
        let wrappee_clone = self.wrappee.clone();
        let log_storage = self.proxy_handler.log_storage.clone();
        tokio::spawn(async move {
            loop {
                let mut wrappee_guard = wrappee_clone.write().await;
                if let Some(wrappee) = wrappee_guard.as_mut() {
                    if let Ok(Some(stderr_msg)) = wrappee.receive_stderr().await {
                        log_storage.add_stderr(stderr_msg).await;
                    }
                }
                drop(wrappee_guard);
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        });

        Ok(())
    }

    // Dynamic tool handler - not directly exposed through tool_router
    pub async fn call_tool_dynamic(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<CallToolResult, McpError> {
        // Handle built-in tools
        if name == "show_log" {
            let req: show_log::ShowLogRequest =
                serde_json::from_value(arguments).map_err(|e| McpError {
                    code: ErrorCode::INVALID_PARAMS,
                    message: format!("Invalid parameters: {e}").into(),
                    data: None,
                })?;
            return show_log::show_log(req, &self.proxy_handler.log_storage).await;
        }

        if name == "clear_log" {
            let req: show_log::ClearLogRequest =
                serde_json::from_value(arguments).map_err(|e| McpError {
                    code: ErrorCode::INVALID_PARAMS,
                    message: format!("Invalid parameters: {e}").into(),
                    data: None,
                })?;
            return show_log::clear_log(req, &self.proxy_handler.log_storage).await;
        }

        // Proxy to wrappee
        let mut wrappee_guard = self.wrappee.write().await;
        if let Some(wrappee) = wrappee_guard.as_mut() {
            self.proxy_handler
                .proxy_tool_call(name, arguments, wrappee)
                .await
        } else {
            Err(McpError {
                code: ErrorCode::INTERNAL_ERROR,
                message: "Wrappee not initialized".into(),
                data: None,
            })
        }
    }
}

impl ServerHandler for WrapServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "Wrap-MCP".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            instructions: Some(
                "This is a transparent MCP wrapper that logs all requests/responses while proxying to a wrapped MCP server."
                    .to_string(),
            ),
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self.proxy_handler.get_all_tools().await;
        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let arguments = request
            .arguments
            .map(serde_json::Value::Object)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        self.call_tool_dynamic(&request.name, arguments).await
    }
}
