use anyhow::Result;
use rmcp::{
    RoleServer, Service, ServiceExt,
    transport::{
        stdio,
        streamable_http_server::{StreamableHttpService, session::local::LocalSessionManager},
    },
};

pub async fn run_stdio_server<S>(
    service_factory: impl Fn() -> Result<S, std::io::Error> + Send + Sync + 'static,
) -> Result<()>
where
    S: Service<RoleServer> + Send + 'static,
{
    tracing::info!("Initializing stdio transport");

    let service = service_factory()?.serve(stdio()).await.inspect_err(|e| {
        tracing::error!("Server error: {:?}", e);
    })?;

    tracing::info!("Server started successfully on stdio transport");
    service.waiting().await?;

    tracing::info!("Server shutting down");
    Ok(())
}

pub async fn run_http_server<S>(
    service_factory: impl Fn() -> Result<S, std::io::Error> + Send + Sync + 'static,
) -> Result<()>
where
    S: Service<RoleServer> + Send + 'static,
{
    const BIND_ADDRESS: &str = "127.0.0.1:8000";

    tracing::info!("Initializing streamable HTTP transport on {BIND_ADDRESS}");

    let service = StreamableHttpService::new(
        service_factory,
        LocalSessionManager::default().into(),
        Default::default(),
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let tcp_listener = tokio::net::TcpListener::bind(BIND_ADDRESS).await?;

    tracing::info!(
        "Server started successfully on streamable HTTP transport at http://{BIND_ADDRESS}/mcp",
    );

    axum::serve(tcp_listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.unwrap();
            tracing::info!("Received shutdown signal");
        })
        .await?;

    tracing::info!("Server shutting down");
    Ok(())
}
