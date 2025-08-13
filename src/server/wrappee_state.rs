use crate::{
    config::WrappeeConfig,
    wrappee::WrappeeClient,
};
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

    /// Check if command configuration exists
    pub async fn has_config(&self) -> bool {
        let cmd = self.command.read().await;
        let args = self.args.read().await;
        cmd.is_some() && args.is_some()
    }
}