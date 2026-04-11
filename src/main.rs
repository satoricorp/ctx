use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    ctx::cli::run().await
}
