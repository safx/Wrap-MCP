use crate::{
    config::{LogConfig, WrappeeConfig},
    logging::LogStorage,
    server::wrappee::WrappeeController,
    tools::ToolManager,
    wrappee::WrappeeClient,
};
use anyhow::Result;
use rmcp::{RoleServer, service::Peer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{RwLock, mpsc};

#[derive(Clone)]
pub struct WrapServer {
    pub(crate) tool_manager: Arc<ToolManager>,
    pub(crate) wrappee_controller: Arc<WrappeeController>,
    pub(crate) peer: Arc<RwLock<Option<Peer<RoleServer>>>>,
    pub(crate) shutting_down: Arc<AtomicBool>,
    pub(crate) shutdown_tx: Arc<RwLock<Option<mpsc::Sender<()>>>>,
}

impl WrapServer {
    pub fn new(log_config: &LogConfig, wrappee_config: &WrappeeConfig) -> Self {
        let log_storage = Arc::new(LogStorage::new(log_config));
        let tool_manager = Arc::new(ToolManager::new(log_storage));

        let wrappee_controller = Arc::new(WrappeeController::new(wrappee_config));

        Self {
            tool_manager,
            wrappee_controller,
            peer: Arc::new(RwLock::new(None)),
            shutting_down: Arc::new(AtomicBool::new(false)),
            shutdown_tx: Arc::new(RwLock::new(None)),
        }
    }

    /// Setup signal handlers for graceful shutdown with a delay to avoid premature shutdown
    pub fn setup_signal_handlers_delayed(&self) {
        let server = self.clone();
        tokio::spawn(async move {
            // Wait for 500ms to ensure initialization is complete
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            server.setup_signal_handlers_internal();
        });
    }

    /// Internal method to setup signal handlers
    fn setup_signal_handlers_internal(&self) {
        let server = self.clone();

        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};

            tokio::spawn(async move {
                let mut sigterm = match signal(SignalKind::terminate()) {
                    Ok(sig) => sig,
                    Err(e) => return tracing::error!("Failed to listen for SIGTERM: {e}"),
                };

                let mut sigint = match signal(SignalKind::interrupt()) {
                    Ok(sig) => sig,
                    Err(e) => return tracing::error!("Failed to listen for SIGINT: {e}"),
                };

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

        // Send shutdown signal to stderr monitoring
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(()).await;
        }

        // Shutdown wrappee
        if let Err(e) = self.wrappee_controller.shutdown().await {
            tracing::warn!("Error shutting down wrappee: {}", e);
        }
    }

    /// Internal method to start a wrappee process with common initialization logic
    pub(crate) async fn start_wrappee_internal(
        &self,
        command: &str,
        args: &[String],
        disable_colors: bool,
    ) -> Result<WrappeeClient> {
        // Delegate to WrappeeController
        self.wrappee_controller
            .start_wrappee(command, args, disable_colors, &self.tool_manager)
            .await
    }

    /// Start stderr monitoring for the wrappee
    pub(crate) fn start_stderr_monitoring(&self) {
        let wrappee_controller = self.wrappee_controller.clone();
        let log_storage = self.tool_manager.log_storage.clone();
        let shutdown_tx = self.shutdown_tx.clone();

        // Create shutdown channel for this monitoring task
        let (tx, mut rx) = mpsc::channel::<()>(1);

        tokio::spawn(async move {
            // Store the shutdown sender
            *shutdown_tx.write().await = Some(tx);

            loop {
                // Check for stderr messages without holding the lock during async operations
                let stderr_result = {
                    let mut wrappee_guard = wrappee_controller.get_client_mut().await;
                    if let Some(wrappee) = wrappee_guard.as_mut() {
                        // Try non-blocking receive first
                        wrappee.receive_stderr().await
                    } else {
                        Ok(None)
                    }
                    // Lock is released here
                };

                // Process the result without holding the lock
                match stderr_result {
                    Ok(Some(stderr_msg)) => {
                        log_storage.add_stderr(stderr_msg).await;
                    }
                    Ok(None) => {
                        // No message available, wait a bit or for shutdown
                        tokio::select! {
                            _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                                // Continue checking
                            }
                            _ = rx.recv() => {
                                tracing::info!("Stderr monitoring received shutdown signal");
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error receiving stderr: {}", e);
                        break;
                    }
                }
            }

            tracing::info!("Stderr monitoring task ended");
        });
    }

    /// Get PID of current wrappee process
    pub(crate) async fn get_wrappee_pid(&self) -> Option<u32> {
        self.wrappee_controller.get_pid().await
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
}
