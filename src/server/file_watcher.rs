use super::wrap_server::WrapServer;
use anyhow::Result;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};

impl WrapServer {
    pub(crate) async fn start_file_watching(&self) -> Result<()> {
        // Only clone if we actually have a command to watch
        let binary_path = {
            let command_guard = self.wrappee_controller.command.read().await;
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
                                    let has_existing_wrappee = server.wrappee_controller.is_active().await;

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
                                        if let Some((cmd, args, disable_colors)) = server.wrappee_controller.get_command().await {
                                            // Start the wrappee
                                            match server.start_wrappee_internal(&cmd, &args, disable_colors).await {
                                                Ok(wrappee_client) => {
                                                    server.wrappee_controller.set_client(Some(wrappee_client)).await;
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
}
