use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    ctx::api::run_server_from_env().await
}

