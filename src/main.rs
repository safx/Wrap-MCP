use anyhow::Result;
use wrap_mcp::{server, WrapServer};
use std::env;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    tracing::info!("Starting Wrap MCP Server");

    let transport = env::var("WRAP_MCP_TRANSPORT").unwrap_or_else(|_| "stdio".to_string());

    let service_factory = || {
        tracing::info!("Creating service instance");
        Ok(WrapServer::new())
    };

    match transport.as_str() {
        "stdio" => server::run_stdio_server(service_factory).await,
        "streamable-http" | "http" => server::run_http_server(service_factory).await,
        _ => {
            tracing::error!("Unknown transport: {}", transport);
            anyhow::bail!(
                "Unknown transport: {}. Use 'stdio' or 'streamable-http'",
                transport
            )
        }
    }
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_line_number(true)
        .with_file(true)
        .with_writer(std::io::stderr);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}
