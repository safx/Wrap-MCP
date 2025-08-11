use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio::task;
use tokio::time::{Duration, timeout};
use crate::config::Config;

#[derive(Debug)]
pub struct WrappeeClient {
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<std::process::ChildStdin>>,
    stdout_rx: mpsc::Receiver<String>,
    stderr_rx: mpsc::Receiver<String>,
    timeout_duration: Duration,
}

impl WrappeeClient {
    pub fn spawn(command: &str, args: &[String], disable_colors: bool) -> Result<Self> {
        tracing::info!("Spawning wrappee process: {command} {args:?}");

        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set environment variables to disable colors if requested
        if disable_colors {
            cmd.env("NO_COLOR", "1")
                .env("CLICOLOR", "0")
                .env("RUST_LOG_STYLE", "never");
            tracing::debug!("Setting NO_COLOR=1, CLICOLOR=0, RUST_LOG_STYLE=never for wrappee");
        }

        let mut child = cmd.spawn().context("Failed to spawn wrappee process")?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let stderr = child.stderr.take().context("Failed to get stderr")?;

        let (stdout_tx, stdout_rx) = mpsc::channel(100);
        let (stderr_tx, stderr_rx) = mpsc::channel(100);

        // Spawn stdout reader
        task::spawn_blocking(move || {
            let reader = BufReader::new(stdout);
            tracing::debug!("Starting stdout reader");
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        tracing::debug!("Read line from wrappee stdout: {line}");
                        if stdout_tx.blocking_send(line).is_err() {
                            tracing::error!("Failed to send line to channel");
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error reading stdout: {e}");
                        break;
                    }
                }
            }
            tracing::debug!("Stdout reader finished");
        });

        // Spawn stderr reader
        task::spawn_blocking(move || {
            let reader = BufReader::new(stderr);
            tracing::debug!("Starting stderr reader");
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        tracing::info!("Wrappee stderr: {line}");
                        if stderr_tx.blocking_send(line).is_err() {
                            tracing::error!("Failed to send stderr line to channel");
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error reading stderr: {e}");
                        break;
                    }
                }
            }
            tracing::debug!("Stderr reader finished");
        });

        // Get timeout from config
        let config = Config::global();
        let timeout_secs = config.tool_timeout_secs;

        tracing::info!("Tool timeout set to {timeout_secs} seconds");

        Ok(Self {
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
            stdout_rx,
            stderr_rx,
            timeout_duration: Duration::from_secs(timeout_secs),
        })
    }

    pub async fn send_request(&mut self, request: Value) -> Result<()> {
        let request_str = serde_json::to_string(&request)?;
        tracing::debug!("Sending request to wrappee: {request_str}");

        let mut stdin = self.stdin.lock().await;
        writeln!(stdin, "{request_str}")?;
        stdin.flush()?;

        Ok(())
    }

    pub async fn receive_response(&mut self) -> Result<Option<Value>> {
        if let Ok(line) = self.stdout_rx.try_recv() {
            tracing::debug!("Received response from wrappee: {line}");
            let response: Value = serde_json::from_str(&line)?;
            return Ok(Some(response));
        }
        Ok(None)
    }

    pub async fn receive_stderr(&mut self) -> Result<Option<String>> {
        if let Ok(line) = self.stderr_rx.try_recv() {
            tracing::debug!("Received stderr from wrappee: {line}");
            return Ok(Some(line));
        }
        Ok(None)
    }

    pub async fn wait_for_response(&mut self) -> Result<Value> {
        let timeout_duration = self.timeout_duration.as_secs();
        tracing::debug!(
            "Waiting for response from wrappee (timeout: {timeout_duration} seconds)...",
        );

        match timeout(self.timeout_duration, self.stdout_rx.recv()).await {
            Ok(Some(line)) => {
                tracing::debug!("Received response from wrappee: {line}");
                let response: Value = serde_json::from_str(&line)?;
                Ok(response)
            }
            Ok(None) => {
                tracing::error!("Channel closed - no more messages available");
                anyhow::bail!("Wrappee stdout closed unexpectedly")
            }
            Err(_) => {
                tracing::error!("Tool call timed out after {timeout_duration} seconds",);
                anyhow::bail!("Tool call timed out after {timeout_duration} seconds",)
            }
        }
    }

    pub async fn initialize(&mut self) -> Result<Value> {
        // Get protocol version from config
        let config = Config::global();
        let protocol_version = &config.protocol_version;

        tracing::info!("Initializing wrappee with protocol version: {protocol_version}",);

        let init_request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": protocol_version,
                "capabilities": {},
                "clientInfo": {
                    "name": "wrap-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });

        self.send_request(init_request).await?;
        let response = self.wait_for_response().await?;

        // Send initialized notification
        let initialized_notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        self.send_request(initialized_notification).await?;

        Ok(response)
    }

    pub async fn list_tools(&mut self) -> Result<Value> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });

        self.send_request(request).await?;
        self.wait_for_response().await
    }

    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
        tracing::info!(
            "Calling tool '{name}' with timeout {timeout_duration} seconds",
            timeout_duration = self.timeout_duration.as_secs()
        );

        let request = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments
            }
        });

        self.send_request(request).await?;

        self.wait_for_response()
            .await
            .with_context(|| format!("Tool '{name}' execution failed"))
    }

    pub async fn get_pid(&self) -> Option<u32> {
        let child = self.child.lock().await;
        Some(child.id())
    }

    pub async fn shutdown(self) -> Result<()> {
        let mut child = self.child.lock().await;
        child.kill()?;
        child.wait()?;
        Ok(())
    }
}
