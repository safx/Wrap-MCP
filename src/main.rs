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
    let server = WrapServer::new(&config.log, &config.wrappee);

    // Setup signal handlers with a delay to avoid premature shutdown during initialization
    server.setup_signal_handlers_delayed();

    let service_factory = move || {
        tracing::info!("Creating service instance");

        // Initialize wrappee in the background
        let server_init = server.clone();
        tokio::spawn(async move {
            _ = server_init.initialize_wrappee().await;
        });

        Ok(server.clone())
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
