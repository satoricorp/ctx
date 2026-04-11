use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "ctx-server")]
struct ServerArgs {
    #[arg(long = "host")]
    host: Option<String>,
    #[arg(long = "port")]
    port: Option<u16>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = ServerArgs::parse();
    let host = args
        .host
        .unwrap_or_else(|| std::env::var("CTX_HOST").unwrap_or_else(|_| String::from("0.0.0.0")));
    let port = args
        .port
        .or_else(|| std::env::var("CTX_PORT").ok().and_then(|value| value.parse().ok()))
        .or_else(|| std::env::var("PORT").ok().and_then(|value| value.parse().ok()))
        .unwrap_or(8080);

    ctx::api::run_server(&host, port).await
}
