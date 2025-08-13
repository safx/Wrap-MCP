use crate::{
    config::{LogConfig, WrappeeConfig},
    logging::LogStorage,
    tools::ToolManager,
    wrappee::WrappeeClient,
};
use anyhow::Result;
use rmcp::{ErrorData as McpError, RoleServer, model::*, service::Peer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct WrapServer {
    pub(crate) tool_manager: Arc<ToolManager>,
    pub(crate) wrappee: Arc<RwLock<Option<WrappeeClient>>>,
    pub(crate) wrappee_command: Arc<RwLock<Option<String>>>,
    pub(crate) wrappee_args: Arc<RwLock<Option<Vec<String>>>>,
    pub(crate) disable_colors: Arc<RwLock<bool>>,
    pub(crate) peer: Arc<RwLock<Option<Peer<RoleServer>>>>,
    pub(crate) shutting_down: Arc<AtomicBool>,
    pub(crate) wrappee_config: Arc<WrappeeConfig>,
}

impl WrapServer {
    pub fn new(log_config: &LogConfig, wrappee_config: &WrappeeConfig) -> Self {
        let log_storage = Arc::new(LogStorage::new(log_config));
        let tool_manager = Arc::new(ToolManager::new(log_storage));

        Self {
            tool_manager,
            wrappee: Arc::new(RwLock::new(None)),
            wrappee_command: Arc::new(RwLock::new(None)),
            wrappee_args: Arc::new(RwLock::new(None)),
            disable_colors: Arc::new(RwLock::new(false)),
            peer: Arc::new(RwLock::new(None)),
            shutting_down: Arc::new(AtomicBool::new(false)),
            wrappee_config: Arc::new(wrappee_config.clone()),
        }
    }

    /// Setup signal handlers for graceful shutdown
    pub fn setup_signal_handlers(&self) {
        let server = self.clone();

        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};

            tokio::spawn(async move {
                let mut sigterm =
                    signal(SignalKind::terminate()).expect("Failed to listen for SIGTERM");
                let mut sigint =
                    signal(SignalKind::interrupt()).expect("Failed to listen for SIGINT");

                tokio::select! {
                    _ = sigterm.recv() => {
                        tracing::info!("Received SIGTERM");
                        Self::handle_shutdown_signal(server).await;
                    }
                    _ = sigint.recv() => {
                        tracing::info!("Received SIGINT");
                        Self::handle_shutdown_signal(server).await;
                    }
                }
            });
        }

        #[cfg(not(unix))]
        {
            let server = self.clone();
            tokio::spawn(async move {
                match tokio::signal::ctrl_c().await {
                    Ok(()) => {
                        tracing::info!("Received Ctrl+C");
                        Self::handle_shutdown_signal(server).await;
                    }
                    Err(err) => {
                        tracing::error!("Unable to listen for shutdown signal: {}", err);
                    }
                }
            });
        }
    }

    /// Handle the shutdown signal
    async fn handle_shutdown_signal(server: WrapServer) {
        server.shutdown().await;
        // Give some time for graceful shutdown
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        std::process::exit(0);
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
    pub(crate) async fn start_wrappee_internal(
        &self,
        command: &str,
        args: &[String],
        disable_colors: bool,
    ) -> Result<WrappeeClient> {
        tracing::info!("Starting wrappee process: {command} {args:?}");

        // Spawn the wrappee process
        let mut wrappee_client = WrappeeClient::spawn(
            command,
            args,
            disable_colors,
            self.wrappee_config.as_ref().clone(),
        )?;

        // Initialize the wrappee
        wrappee_client
            .initialize(&self.wrappee_config.protocol_version)
            .await?;

        // Discover tools from wrappee
        self.tool_manager
            .discover_tools(&mut wrappee_client)
            .await?;

        Ok(wrappee_client)
    }

    /// Start stderr monitoring for the wrappee
    pub(crate) fn start_stderr_monitoring(&self) {
        let wrappee_clone = self.wrappee.clone();
        let log_storage = self.tool_manager.log_storage.clone();
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
    pub(crate) async fn get_wrappee_pid(&self) -> Option<u32> {
        let wrappee_guard = self.wrappee.read().await;
        if let Some(wrappee) = wrappee_guard.as_ref() {
            wrappee.get_pid().await
        } else {
            None
        }
    }

    /// Send tool list changed notification if peer is available
    pub(crate) async fn notify_tools_changed(&self) {
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
    pub(crate) fn error_to_mcp(e: impl std::fmt::Display, message: &str) -> McpError {
        McpError {
            code: ErrorCode::INTERNAL_ERROR,
            message: format!("{message}: {e}").into(),
            data: None,
        }
    }
}
