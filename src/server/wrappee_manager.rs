use super::wrap_server::WrapServer;
use crate::tools::{
    clear_log::{ClearLogRequest, clear_log},
    show_log::{ShowLogRequest, show_log},
};
use anyhow::Result;
use rmcp::{ErrorData as McpError, model::*};
use serde_json::Value;

impl WrapServer {
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
            self.tool_manager.log_storage.set_ansi_removal(false).await;
        } else {
            tracing::info!("ANSI escape sequence removal enabled (default)");
            // Store the flag in log storage (true = remove ANSI)
            self.tool_manager.log_storage.set_ansi_removal(true).await;
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
        self.tool_manager.clear_tools().await;

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
            return show_log(req, &self.tool_manager.log_storage).await;
        }

        if name == "clear_log" {
            let req: ClearLogRequest = serde_json::from_value(arguments).map_err(|e| McpError {
                code: ErrorCode::INVALID_PARAMS,
                message: format!("Invalid parameters: {e}").into(),
                data: None,
            })?;
            return clear_log(req, &self.tool_manager.log_storage).await;
        }

        // Proxy to wrappee
        let mut wrappee_guard = self.wrappee.write().await;
        if let Some(wrappee) = wrappee_guard.as_mut() {
            self.tool_manager
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
