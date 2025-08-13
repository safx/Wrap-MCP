use crate::{config::WrappeeConfig, tools::ToolManager, wrappee::WrappeeClient};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages the state and lifecycle of a wrappee process
#[derive(Clone)]
pub struct WrappeeState {
    /// The active wrappee client connection
    pub(crate) client: Arc<RwLock<Option<WrappeeClient>>>,

    /// Command used to spawn the wrappee
    pub(crate) command: Arc<RwLock<Option<String>>>,

    /// Arguments passed to the wrappee command
    pub(crate) args: Arc<RwLock<Option<Vec<String>>>>,

    /// Whether to disable colors in wrappee output
    pub(crate) disable_colors: Arc<RwLock<bool>>,

    /// Configuration for the wrappee
    pub(crate) config: Arc<WrappeeConfig>,
}

impl WrappeeState {
    pub fn new(wrappee_config: &WrappeeConfig) -> Self {
        Self {
            client: Arc::new(RwLock::new(None)),
            command: Arc::new(RwLock::new(None)),
            args: Arc::new(RwLock::new(None)),
            disable_colors: Arc::new(RwLock::new(false)),
            config: Arc::new(wrappee_config.clone()),
        }
    }

    /// Store the command and arguments for the wrappee
    pub async fn set_command(&self, command: String, args: Vec<String>, disable_colors: bool) {
        *self.command.write().await = Some(command);
        *self.args.write().await = Some(args);
        *self.disable_colors.write().await = disable_colors;
    }

    /// Get the stored command and arguments
    pub async fn get_command(&self) -> Option<(String, Vec<String>, bool)> {
        let cmd = self.command.read().await;
        let args = self.args.read().await;
        let disable_colors = *self.disable_colors.read().await;

        match (cmd.as_ref(), args.as_ref()) {
            (Some(c), Some(a)) => Some((c.clone(), a.clone(), disable_colors)),
            _ => None,
        }
    }

    /// Set the active wrappee client
    pub async fn set_client(&self, client: Option<WrappeeClient>) {
        *self.client.write().await = client;
    }

    /// Get a mutable reference to the wrappee client
    pub async fn get_client_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, Option<WrappeeClient>> {
        self.client.write().await
    }

    /// Take the wrappee client (removes it from state)
    pub async fn take_client(&self) -> Option<WrappeeClient> {
        self.client.write().await.take()
    }

    /// Check if a wrappee is currently active
    pub async fn is_active(&self) -> bool {
        self.client.read().await.is_some()
    }

    /// Get PID of current wrappee process
    pub async fn get_pid(&self) -> Option<u32> {
        let client_guard = self.client.read().await;
        if let Some(client) = client_guard.as_ref() {
            client.get_pid().await
        } else {
            None
        }
    }

    /// Start a wrappee process with the stored configuration
    pub async fn start_wrappee(
        &self,
        command: &str,
        args: &[String],
        disable_colors: bool,
        tool_manager: &ToolManager,
    ) -> Result<WrappeeClient> {
        tracing::info!("Starting wrappee process: {command} {args:?}");

        // Spawn the wrappee process
        let mut wrappee_client =
            WrappeeClient::spawn(command, args, disable_colors, self.config.as_ref().clone())?;

        // Initialize the wrappee
        wrappee_client
            .initialize(&self.config.protocol_version)
            .await?;

        // Discover tools from wrappee
        tool_manager.discover_tools(&mut wrappee_client).await?;

        Ok(wrappee_client)
    }

    /// Shutdown the current wrappee process
    pub async fn shutdown(&self) -> Result<()> {
        if let Some(client) = self.take_client().await {
            tracing::info!("Shutting down wrappee process");
            client.shutdown().await?;
        }
        Ok(())
    }

    /// Initialize and start wrappee from command configuration
    pub async fn initialize(
        &self,
        command: &str,
        args: &[String],
        disable_colors: bool,
        tool_manager: &ToolManager,
    ) -> Result<()> {
        // Store configuration for potential restart
        self.set_command(command.to_string(), args.to_vec(), disable_colors)
            .await;

        // Start the wrappee
        let client = self
            .start_wrappee(command, args, disable_colors, tool_manager)
            .await?;

        // Store the client
        self.set_client(Some(client)).await;

        Ok(())
    }

    /// Restart the wrappee with stored configuration
    pub async fn restart(&self, tool_manager: &ToolManager) -> Result<()> {
        // Check if configuration exists
        let (command, args, disable_colors) = self
            .get_command()
            .await
            .ok_or_else(|| anyhow::anyhow!("No wrappee configuration available for restart"))?;

        // Shutdown existing wrappee
        if let Err(e) = self.shutdown().await {
            tracing::warn!("Error during shutdown: {e}");
        }

        // Wait for clean shutdown
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Clear tools before restarting
        tool_manager.clear_tools().await;

        // Start new wrappee
        let client = self
            .start_wrappee(&command, &args, disable_colors, tool_manager)
            .await?;

        // Store the new client
        self.set_client(Some(client)).await;

        tracing::info!("Wrappee restarted successfully");
        Ok(())
    }

    /// Proxy a tool call to the wrappee
    pub async fn proxy_tool_call(
        &self,
        name: &str,
        arguments: serde_json::Value,
        tool_manager: &ToolManager,
    ) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
        let mut wrappee_guard = self.get_client_mut().await;
        if let Some(wrappee) = wrappee_guard.as_mut() {
            tool_manager.proxy_tool_call(name, arguments, wrappee).await
        } else {
            Err(rmcp::ErrorData {
                code: rmcp::model::ErrorCode::INTERNAL_ERROR,
                message: "Wrappee not initialized".into(),
                data: None,
            })
        }
    }
}
