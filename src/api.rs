pub mod routes;

use anyhow::Result;
use axum::serve;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub async fn start_api(port: u16) -> Result<()> {
    let host = std::env::var("CTX_HOST").unwrap_or_else(|_| String::from("0.0.0.0"));
    run_server(&host, port).await
}

pub async fn run_server(host: &str, port: u16) -> Result<()> {
    let app = routes::router()
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
    let listener = TcpListener::bind(addr).await?;
    println!("ctx: api listening on {}", addr);
    serve(listener, app).await?;
    Ok(())
}

pub async fn run_server_from_env() -> Result<()> {
    let port = std::env::var("CTX_PORT")
        .ok()
        .or_else(|| std::env::var("PORT").ok())
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);

    start_api(port).await
}
