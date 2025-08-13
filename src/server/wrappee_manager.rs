use super::wrap_server::WrapServer;
use crate::{
    cli::CliOptions,
    tools::{
        clear_log::{ClearLogRequest, clear_log},
        show_log::{ShowLogRequest, show_log},
    },
};
use anyhow::Result;
use rmcp::{ErrorData as McpError, model::*};
use serde_json::Value;

impl WrapServer {
    pub async fn initialize_wrappee(&self) -> Result<()> {
        // Parse command line arguments
        let opts = CliOptions::from_args();

        // Configure ANSI removal
        self.tool_manager
            .log_storage
            .set_ansi_removal(!opts.preserve_ansi)
            .await;

        // Initialize the wrappee
        let init_result = self
            .wrappee_controller
            .initialize(
                &opts.command,
                &opts.args,
                opts.disable_colors(),
                &self.tool_manager,
            )
            .await;

        match init_result {
            Ok(_) => {
                // Start stderr monitoring in the background
                self.start_stderr_monitoring();
            }
            Err(e) => {
                // If not in watch mode, panic on failure to start wrappee
                if !opts.watch_binary {
                    panic!("Failed to spawn wrappee process '{}': {e}", opts.command);
                }
                // In watch mode, log the error but continue to set up file watching
                tracing::warn!("Failed to start wrappee (will wait for file creation): {e}");
            }
        }

        // Start file watching if enabled
        if opts.watch_binary {
            tracing::info!("Binary file watching enabled (-w option)");
            self.start_file_watching().await?;
        }

        Ok(())
    }

    pub async fn restart_wrapped_server(&self) -> Result<CallToolResult, McpError> {
        tracing::info!("Restarting wrapped server");

        // Restart the wrappee
        self.wrappee_controller
            .restart(&self.tool_manager)
            .await
            .map_err(|e| McpError {
                code: ErrorCode::INTERNAL_ERROR,
                message: format!("Failed to restart wrapped server: {e}").into(),
                data: None,
            })?;

        // Send tool list changed notification
        self.notify_tools_changed().await;

        // Restart stderr monitoring
        self.start_stderr_monitoring();

        Ok(CallToolResult::success(vec![Content::text(
            "✅ Wrapped server restarted successfully",
        )]))
    }

    /// Handle tool calls - both built-in and proxied tools
    pub async fn handle_tool_call(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<CallToolResult, McpError> {
        // Handle built-in tools
        match name {
            "restart_wrapped_server" => self.restart_wrapped_server().await,
            "show_log" => {
                let req: ShowLogRequest =
                    serde_json::from_value(arguments).map_err(|e| McpError {
                        code: ErrorCode::INVALID_PARAMS,
                        message: format!("Invalid parameters: {e}").into(),
                        data: None,
                    })?;
                show_log(req, &self.tool_manager.log_storage).await
            }
            "clear_log" => {
                let req: ClearLogRequest =
                    serde_json::from_value(arguments).map_err(|e| McpError {
                        code: ErrorCode::INVALID_PARAMS,
                        message: format!("Invalid parameters: {e}").into(),
                        data: None,
                    })?;
                clear_log(req, &self.tool_manager.log_storage).await
            }
            _ => {
                // Proxy to wrappee
                self.wrappee_controller
                    .proxy_tool_call(name, arguments, &self.tool_manager)
                    .await
            }
        }
    }
}
