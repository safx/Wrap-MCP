use anyhow::Result;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};
use wrap_mcp::{WrapServer, config::Config, run};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize configuration first
    Config::initialize()?;
    let config = Config::global();

    init_tracing(config);

    tracing::info!("Starting Wrap MCP Server");

    let transport = &config.transport;

    // Create a shared server instance for signal handling
    let server = WrapServer::new();
    let server_for_signal = server.clone();

    // Setup signal handlers
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
                    server_for_signal.shutdown().await;
                    // Give some time for graceful shutdown
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    std::process::exit(0);
                }
                _ = sigint.recv() => {
                    tracing::info!("Received SIGINT");
                    server_for_signal.shutdown().await;
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    std::process::exit(0);
                }
            }
        });
    }

    #[cfg(not(unix))]
    {
        let server_for_signal = server.clone();
        tokio::spawn(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    tracing::info!("Received Ctrl+C");
                    server_for_signal.shutdown().await;
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    std::process::exit(0);
                }
                Err(err) => {
                    tracing::error!("Unable to listen for shutdown signal: {}", err);
                }
            }
        });
    }

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
        "stdio" => run::run_stdio_server(service_factory).await,
        "streamable-http" | "http" => run::run_http_server(service_factory).await,
        _ => {
            tracing::error!("Unknown transport: {transport}");
            anyhow::bail!("Unknown transport: {transport}. Use 'stdio' or 'streamable-http'",)
        }
    }
}

fn init_tracing(config: &Config) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.rust_log));

    // Use ANSI colors from config
    let enable_ansi = config.log_colors;

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
