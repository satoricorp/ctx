pub mod routes;

use anyhow::Result;

pub async fn start_api(_port: u16) -> Result<()> {
    anyhow::bail!("http api is not implemented yet")
}

pub async fn run_server_from_env() -> Result<()> {
    let port = std::env::var("CTX_PORT")
        .ok()
        .or_else(|| std::env::var("PORT").ok())
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);

    start_api(port).await
}

