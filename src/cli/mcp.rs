use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
#[command(about = "Start the local MCP server")]
pub struct McpArgs {
    /// Port to listen on.
    #[arg(long = "port", default_value_t = 3000)]
    pub port: u16,
}

pub async fn run(args: McpArgs) -> Result<()> {
    crate::mcp::start_mcp_server(args.port).await
}
