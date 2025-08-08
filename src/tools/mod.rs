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
    wrappee_command: Arc<RwLock<Option<String>>>,
    wrappee_args: Arc<RwLock<Option<Vec<String>>>>,
    disable_colors: Arc<RwLock<bool>>,
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
            wrappee_command: Arc::new(RwLock::new(None)),
            wrappee_args: Arc::new(RwLock::new(None)),
            disable_colors: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn initialize_wrappee(&self) -> Result<()> {
        // Parse command line arguments
        let args: Vec<String> = std::env::args().collect();

        // Find the "--" separator
        let separator_pos = args.iter().position(|arg| arg == "--");
        
        // Check for --ansi option before "--"
        let preserve_ansi = separator_pos
            .map(|pos| args[1..pos].contains(&"--ansi".to_string()))
            .unwrap_or(false);
        
        if preserve_ansi {
            tracing::info!("ANSI escape sequences will be preserved (--ansi option)");
            // Store the flag in log storage (false = don't remove ANSI)
            self.proxy_handler.log_storage.set_ansi_removal(false).await;
        } else {
            tracing::info!("ANSI escape sequence removal enabled (default)");
            // Store the flag in log storage (true = remove ANSI)
            self.proxy_handler.log_storage.set_ansi_removal(true).await;
        }

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

        // Store command and args for potential restart
        *self.wrappee_command.write().await = Some(command.clone());
        *self.wrappee_args.write().await = Some(wrappee_args.clone());
        *self.disable_colors.write().await = !preserve_ansi;
        
        // Pass !preserve_ansi to disable colors (we want to disable colors by default)
        let mut wrappee_client = WrappeeClient::spawn(&command, &wrappee_args, !preserve_ansi)?;

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

    pub async fn restart_wrapped_server(&self) -> Result<CallToolResult, McpError> {
        tracing::info!("Restarting wrapped server");
        
        // Get stored command and args
        let command = self.wrappee_command.read().await.clone();
        let args = self.wrappee_args.read().await.clone();
        let disable_colors = *self.disable_colors.read().await;
        
        let (command, args) = match (command, args) {
            (Some(cmd), Some(args)) => (cmd, args),
            _ => {
                return Err(McpError {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: "No wrapped server to restart".into(),
                    data: None,
                });
            }
        };
        
        // Shutdown existing wrappee
        {
            let mut wrappee_guard = self.wrappee.write().await;
            if let Some(client) = wrappee_guard.take() {
                tracing::info!("Shutting down existing wrapped server");
                if let Err(e) = client.shutdown().await {
                    tracing::warn!("Error during shutdown: {}", e);
                }
            }
        }
        
        // Wait a bit for clean shutdown
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        // Start new wrappee
        tracing::info!("Starting new wrapped server: {} {:?}", command, args);
        let mut wrappee_client = WrappeeClient::spawn(&command, &args, disable_colors)
            .map_err(|e| McpError {
                code: ErrorCode::INTERNAL_ERROR,
                message: format!("Failed to spawn wrapped server: {}", e).into(),
                data: None,
            })?;
        
        // Initialize the wrappee
        wrappee_client.initialize().await
            .map_err(|e| McpError {
                code: ErrorCode::INTERNAL_ERROR,
                message: format!("Failed to initialize wrapped server: {}", e).into(),
                data: None,
            })?;
        
        // Clear and rediscover tools from wrappee
        self.proxy_handler.clear_tools().await;
        self.proxy_handler
            .discover_tools(&mut wrappee_client)
            .await
            .map_err(|e| McpError {
                code: ErrorCode::INTERNAL_ERROR,
                message: format!("Failed to discover tools: {}", e).into(),
                data: None,
            })?;
        
        // Store the new wrappee client
        *self.wrappee.write().await = Some(wrappee_client);
        
        // Restart stderr monitoring
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
        
        tracing::info!("Wrapped server restarted successfully");
        Ok(CallToolResult::success(vec![Content::text(
            "✅ Wrapped server restarted successfully"
        )]))
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
        
        if name == "restart_wrapped_server" {
            return self.restart_wrapped_server().await;
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
