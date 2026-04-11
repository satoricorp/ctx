pub mod tools;

use anyhow::Result;

pub async fn start_mcp_server(_port: u16) -> Result<()> {
    anyhow::bail!("mcp server is not implemented yet")
}

