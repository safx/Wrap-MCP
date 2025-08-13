use anyhow::Result;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use wrap_mcp::{
    WrapServer,
    config::{Config, LogConfig},
    server::transport,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration from environment
    let config = Config::from_env()?;

    init_tracing(&config.log);

    tracing::info!("Starting Wrap MCP Server");

    let transport = &config.transport.transport;

    // Create a shared server instance for signal handling
    let server = WrapServer::new(config.log.clone(), config.wrappee.clone());

    // Setup signal handlers
    setup_signal_handlers(server.clone());

    let service_factory = move || {
        tracing::info!("Creating service instance");
        let server_clone = server.clone();

        // Initialize wrappee in the background
        let server_init = server.clone();
        tokio::spawn(async move {
            if let Err(e) = server_init.initialize_wrappee().await {
                tracing::error!("Failed to initialize wrappee: {e}");
            }
        });

        Ok(server_clone)
    };

    match transport.as_str() {
        "stdio" => transport::run_stdio_server(service_factory).await,
        "streamable-http" | "http" => transport::run_http_server(service_factory).await,
        _ => {
            tracing::error!("Unknown transport: {transport}");
            anyhow::bail!("Unknown transport: {transport}. Use 'stdio' or 'streamable-http'",)
        }
    }
}

/// Setup signal handlers for graceful shutdown
fn setup_signal_handlers(server: WrapServer) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        tokio::spawn(async move {
            let mut sigterm =
                signal(SignalKind::terminate()).expect("Failed to listen for SIGTERM");
            let mut sigint = signal(SignalKind::interrupt()).expect("Failed to listen for SIGINT");

            tokio::select! {
                _ = sigterm.recv() => {
                    tracing::info!("Received SIGTERM");
                    handle_shutdown(server).await;
                }
                _ = sigint.recv() => {
                    tracing::info!("Received SIGINT");
                    handle_shutdown(server).await;
                }
            }
        });
    }

    #[cfg(not(unix))]
    {
        tokio::spawn(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    tracing::info!("Received Ctrl+C");
                    handle_shutdown(server).await;
                }
                Err(err) => {
                    tracing::error!("Unable to listen for shutdown signal: {}", err);
                }
            }
        });
    }
}

/// Handle the shutdown process
async fn handle_shutdown(server: WrapServer) {
    server.shutdown().await;
    // Give some time for graceful shutdown
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    std::process::exit(0);
}

fn init_tracing(log_config: &LogConfig) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_config.rust_log));

    // Use ANSI colors from config
    let enable_ansi = log_config.log_colors;

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_file(true)
        .with_ansi(enable_ansi) // Control ANSI colors via env var
        .with_writer(std::io::stderr);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}
