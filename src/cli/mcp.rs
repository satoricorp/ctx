use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct McpArgs {
    #[arg(long = "port", default_value_t = 3000)]
    pub port: u16,
}

pub async fn run(args: McpArgs) -> Result<()> {
    crate::mcp::start_mcp_server(args.port).await
}
