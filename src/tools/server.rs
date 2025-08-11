use crate::tools::clear_log::{ClearLogRequest, clear_log};
use crate::tools::show_log::{ShowLogRequest, show_log};
use crate::{logging::LogStorage, proxy::ProxyHandler, wrappee::WrappeeClient};
use anyhow::Result;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::*,
    service::{Peer, RequestContext},
};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{RwLock, mpsc};
use tokio::time::{Duration, Instant};

#[derive(Clone)]
pub struct WrapServer {
    proxy_handler: Arc<ProxyHandler>,
    wrappee: Arc<RwLock<Option<WrappeeClient>>>,
    wrappee_command: Arc<RwLock<Option<String>>>,
    wrappee_args: Arc<RwLock<Option<Vec<String>>>>,
    disable_colors: Arc<RwLock<bool>>,
    peer: Arc<RwLock<Option<Peer<RoleServer>>>>,
    shutting_down: Arc<AtomicBool>,
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
            peer: Arc::new(RwLock::new(None)),
            shutting_down: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Initiate graceful shutdown
    pub async fn shutdown(&self) {
        tracing::info!("Initiating graceful shutdown");
        self.shutting_down.store(true, Ordering::SeqCst);

        // Shutdown wrappee
        let mut wrappee_guard = self.wrappee.write().await;
        if let Some(client) = wrappee_guard.take() {
            tracing::info!("Shutting down wrappee process");
            if let Err(e) = client.shutdown().await {
                tracing::warn!("Error shutting down wrappee: {}", e);
            }
        }
    }

    /// Internal method to start a wrappee process with common initialization logic
    async fn start_wrappee_internal(
        &self,
        command: &str,
        args: &[String],
        disable_colors: bool,
    ) -> Result<WrappeeClient> {
        tracing::info!("Starting wrappee process: {command} {args:?}");

        // Spawn the wrappee process
        let mut wrappee_client = WrappeeClient::spawn(command, args, disable_colors)?;

        // Initialize the wrappee
        wrappee_client.initialize().await?;

        // Discover tools from wrappee
        self.proxy_handler
            .discover_tools(&mut wrappee_client)
            .await?;

        Ok(wrappee_client)
    }

    /// Start stderr monitoring for the wrappee
    fn start_stderr_monitoring(&self) {
        let wrappee_clone = self.wrappee.clone();
        let log_storage = self.proxy_handler.log_storage.clone();
        tokio::spawn(async move {
            loop {
                let mut wrappee_guard = wrappee_clone.write().await;
                if let Some(wrappee) = wrappee_guard.as_mut()
                    && let Ok(Some(stderr_msg)) = wrappee.receive_stderr().await
                {
                    log_storage.add_stderr(stderr_msg).await;
                }
                drop(wrappee_guard);
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        });
    }

    /// Get PID of current wrappee process
    async fn get_wrappee_pid(&self) -> Option<u32> {
        let wrappee_guard = self.wrappee.read().await;
        if let Some(wrappee) = wrappee_guard.as_ref() {
            wrappee.get_pid().await
        } else {
            None
        }
    }

    /// Send tool list changed notification if peer is available
    async fn notify_tools_changed(&self) {
        if let Some(peer) = self.peer.read().await.as_ref() {
            tracing::info!("Sending tools/list_changed notification to client");
            if let Err(e) = peer.notify_tool_list_changed().await {
                tracing::warn!("Failed to send tool list changed notification: {e}");
            }
        } else {
            tracing::info!("No peer available for tool list changed notification");
        }
    }

    /// Convert anyhow::Error to McpError for tool call responses
    fn error_to_mcp(e: impl std::fmt::Display, message: &str) -> McpError {
        McpError {
            code: ErrorCode::INTERNAL_ERROR,
            message: format!("{message}: {e}").into(),
            data: None,
        }
    }

    pub async fn initialize_wrappee(&self) -> Result<()> {
        // Parse command line arguments
        let args: Vec<String> = std::env::args().collect();

        // Find the "--" separator
        let separator_pos = args.iter().position(|arg| arg == "--");

        // Check for options before "--"
        let (preserve_ansi, watch_binary) = separator_pos
            .map(|pos| {
                let opts = &args[1..pos];
                (
                    opts.contains(&"--ansi".to_string()),
                    opts.contains(&"-w".to_string()),
                )
            })
            .unwrap_or((false, false));

        if preserve_ansi {
            tracing::info!("ANSI escape sequences will be preserved (--ansi option)");
            // Store the flag in log storage (false = don't remove ANSI)
            self.proxy_handler.log_storage.set_ansi_removal(false).await;
        } else {
            tracing::info!("ANSI escape sequence removal enabled (default)");
            // Store the flag in log storage (true = remove ANSI)
            self.proxy_handler.log_storage.set_ansi_removal(true).await;
        }

        if watch_binary {
            tracing::info!("Binary file watching enabled (-w option)");
        }

        let (command, wrappee_args) = match separator_pos {
            Some(pos) if pos + 1 < args.len() => {
                // Get the command and arguments after "--"
                let command = args[pos + 1].clone();
                let wrappee_args = args
                    .get(pos + 2..)
                    .map(|slice| slice.to_vec())
                    .unwrap_or_default();
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

        tracing::info!("Initializing wrappee with command: {command} {wrappee_args:?}");

        // Store command and args for potential restart
        *self.wrappee_command.write().await = Some(command.clone());
        *self.wrappee_args.write().await = Some(wrappee_args.clone());
        *self.disable_colors.write().await = !preserve_ansi;

        // Start the wrappee using the common method
        let wrappee_result = self
            .start_wrappee_internal(&command, &wrappee_args, !preserve_ansi)
            .await;

        match wrappee_result {
            Ok(wrappee_client) => {
                // Store the wrappee client
                *self.wrappee.write().await = Some(wrappee_client);

                // Start stderr monitoring in the background
                self.start_stderr_monitoring();
            }
            Err(e) => {
                // If not in watch mode, panic on failure to start wrappee
                if !watch_binary {
                    panic!("Failed to spawn wrappee process '{command}': {e}");
                }
                // In watch mode, log the error but continue to set up file watching
                tracing::warn!("Failed to start wrappee (will wait for file creation): {e}");
            }
        }

        // Start file watching if enabled (even if wrappee failed to start)
        if watch_binary {
            self.start_file_watching().await?;
        }

        Ok(())
    }

    async fn start_file_watching(&self) -> Result<()> {
        // Only clone if we actually have a command to watch
        let binary_path = {
            let command_guard = self.wrappee_command.read().await;
            command_guard.as_ref().cloned()
        };

        if let Some(binary_path) = binary_path {
            tracing::info!("Starting file watch for: {binary_path}");

            // Channel for file change events
            let (tx, mut rx) = mpsc::channel::<EventKind>(100);

            // Create watcher with custom handler
            let mut watcher = RecommendedWatcher::new(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(event) = res {
                        // Send event kind through channel
                        match event.kind {
                            EventKind::Modify(_) => {
                                let _ = tx.blocking_send(EventKind::Modify(
                                    notify::event::ModifyKind::Any,
                                ));
                            }
                            EventKind::Create(_) => {
                                let _ = tx.blocking_send(EventKind::Create(
                                    notify::event::CreateKind::Any,
                                ));
                            }
                            EventKind::Remove(_) => {
                                let _ = tx.blocking_send(EventKind::Remove(
                                    notify::event::RemoveKind::Any,
                                ));
                            }
                            _ => {}
                        }
                    }
                },
                Config::default(),
            )
            .map_err(|e| {
                // Panic on watcher creation failure
                panic!("Failed to create file watcher: {e}");
            })?;

            // Determine what to watch
            let path_to_watch = Path::new(&binary_path);
            let (watch_path, watching_parent) = if path_to_watch.exists() {
                // File exists, watch it directly
                (path_to_watch.to_path_buf(), false)
            } else {
                // File doesn't exist, watch parent directory
                let parent = path_to_watch.parent().unwrap_or(Path::new("."));
                tracing::info!(
                    "Binary file doesn't exist, watching parent directory: {}",
                    parent.display()
                );
                (parent.to_path_buf(), true)
            };

            // Start watching
            watcher
                .watch(&watch_path, RecursiveMode::NonRecursive)
                .map_err(|e| {
                    panic!("Failed to watch path {}: {e}", watch_path.display());
                })?;

            // Keep watcher alive by storing it
            std::mem::forget(watcher);

            // Spawn debounced restart handler
            let server = self.clone();
            let binary_path_clone = binary_path.clone();
            tokio::spawn(async move {
                let mut last_event = Instant::now();
                let mut pending_restart = false;
                let mut file_deleted = false;
                let mut initial_start_needed = watching_parent; // Need initial start if watching parent

                loop {
                    tokio::select! {
                        Some(event_kind) = rx.recv() => {
                            match event_kind {
                                EventKind::Remove(_) => {
                                    tracing::info!("Binary file removed, waiting for recreation");
                                    file_deleted = true;
                                    pending_restart = false;  // Cancel any pending restart
                                }
                                EventKind::Create(_) if file_deleted || initial_start_needed => {
                                    // Check if the created file is our binary
                                    if std::path::Path::new(&binary_path_clone).exists() {
                                        if initial_start_needed {
                                            tracing::info!("Binary file created for the first time, scheduling initial start");
                                            initial_start_needed = false;
                                        } else {
                                            tracing::info!("Binary file recreated, scheduling restart");
                                        }
                                        file_deleted = false;
                                        last_event = Instant::now();
                                        pending_restart = true;
                                    }
                                }
                                EventKind::Modify(_) if !file_deleted && !initial_start_needed => {
                                    tracing::debug!("Binary file modified, scheduling restart");
                                    last_event = Instant::now();
                                    pending_restart = true;
                                }
                                _ => {}
                            }
                        }
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {
                            // Check if we should restart (2 second debounce)
                            if pending_restart && last_event.elapsed() > Duration::from_secs(2) {
                                // Check if file exists before attempting restart
                                if std::path::Path::new(&binary_path_clone).exists() {
                                    // Check if this is an initial start or a restart
                                    let has_existing_wrappee = server.wrappee.read().await.is_some();

                                    if has_existing_wrappee {
                                        tracing::info!("Binary file change detected, triggering restart after debounce");

                                        // Get PID before restart
                                        let old_pid = server.get_wrappee_pid().await;

                                        // Perform restart
                                        if let Err(e) = server.restart_wrapped_server().await {
                                            tracing::error!("Failed to restart wrapped server: {e:?}");
                                        } else {
                                            // Get new PID after restart
                                            let new_pid = server.get_wrappee_pid().await;
                                            tracing::info!("Automatic restart completed (PID: {old_pid:?} -> {new_pid:?})");
                                        }
                                    } else {
                                        // Initial start - no existing wrappee to shut down
                                        tracing::info!("Binary file now exists, performing initial start");

                                        // Get stored command and args without cloning
                                        let command_guard = server.wrappee_command.read().await;
                                        let args_guard = server.wrappee_args.read().await;
                                        let disable_colors = *server.disable_colors.read().await;

                                        if let (Some(cmd), Some(args)) = (command_guard.as_ref(), args_guard.as_ref()) {
                                            // Start the wrappee
                                            match server.start_wrappee_internal(cmd, args, disable_colors).await {
                                                Ok(wrappee_client) => {
                                                    *server.wrappee.write().await = Some(wrappee_client);
                                                    server.start_stderr_monitoring();

                                                    // Get PID of newly started process
                                                    let new_pid = server.get_wrappee_pid().await;
                                                    tracing::info!("Initial start completed (PID: {new_pid:?})");

                                                    // Send notification if peer is available
                                                    server.notify_tools_changed().await;
                                                }
                                                Err(e) => {
                                                    tracing::error!("Failed to start wrapped server: {e}");
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    tracing::warn!("Binary file does not exist, skipping restart");
                                }

                                pending_restart = false;
                            }
                        }
                    }
                }
            });

            tracing::info!("File watching started successfully");
        } else {
            tracing::warn!("No binary path available for file watching");
        }

        Ok(())
    }

    pub async fn restart_wrapped_server(&self) -> Result<CallToolResult, McpError> {
        tracing::info!("Restarting wrapped server");

        // Check if command and args are available
        let has_config = {
            let cmd_guard = self.wrappee_command.read().await;
            let args_guard = self.wrappee_args.read().await;
            cmd_guard.is_some() && args_guard.is_some()
        };

        if !has_config {
            return Err(McpError {
                code: ErrorCode::INTERNAL_ERROR,
                message: "No wrapped server to restart".into(),
                data: None,
            });
        }

        // Shutdown existing wrappee
        {
            let mut wrappee_guard = self.wrappee.write().await;
            if let Some(client) = wrappee_guard.take() {
                tracing::info!("Shutting down existing wrapped server");
                if let Err(e) = client.shutdown().await {
                    tracing::warn!("Error during shutdown: {e}");
                }
            }
        }

        // Wait a bit for clean shutdown
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Clear tools before restarting
        self.proxy_handler.clear_tools().await;

        // Start new wrappee using stored command and args
        let wrappee_client = {
            let command_guard = self.wrappee_command.read().await;
            let args_guard = self.wrappee_args.read().await;
            let disable_colors = *self.disable_colors.read().await;

            // We checked above that both are Some
            let command = command_guard.as_ref().unwrap();
            let args = args_guard.as_ref().unwrap();

            self.start_wrappee_internal(command, args, disable_colors)
                .await
        }
        .map_err(|e| Self::error_to_mcp(e, "Failed to restart wrapped server"))?;

        // Store the new wrappee client
        *self.wrappee.write().await = Some(wrappee_client);

        // Send tool list changed notification if peer is available
        self.notify_tools_changed().await;

        // Restart stderr monitoring
        self.start_stderr_monitoring();

        tracing::info!("Wrapped server restarted successfully");
        Ok(CallToolResult::success(vec![Content::text(
            "✅ Wrapped server restarted successfully",
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
            let req: ShowLogRequest = serde_json::from_value(arguments).map_err(|e| McpError {
                code: ErrorCode::INVALID_PARAMS,
                message: format!("Invalid parameters: {e}").into(),
                data: None,
            })?;
            return show_log(req, &self.proxy_handler.log_storage).await;
        }

        if name == "clear_log" {
            let req: ClearLogRequest = serde_json::from_value(arguments).map_err(|e| McpError {
                code: ErrorCode::INVALID_PARAMS,
                message: format!("Invalid parameters: {e}").into(),
                data: None,
            })?;
            return clear_log(req, &self.proxy_handler.log_storage).await;
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
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
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
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        // Store peer for future notifications
        *self.peer.write().await = Some(context.peer.clone());

        let tools = self.proxy_handler.get_all_tools().await;
        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        // Store peer for future notifications
        *self.peer.write().await = Some(context.peer.clone());

        let arguments = request
            .arguments
            .map(serde_json::Value::Object)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        // Handle restart_wrapped_server
        if request.name == "restart_wrapped_server" {
            self.restart_wrapped_server().await
        } else {
            self.call_tool_dynamic(&request.name, arguments).await
        }
    }
}
