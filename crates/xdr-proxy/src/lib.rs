use axum::{routing::get, Router};
use std::net::SocketAddr;
use tracing::info;

pub async fn run_server(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/health", get(health_check));

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("🚀 XDR Proxy listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> &'static str {
    "OK"
}