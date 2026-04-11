use anyhow::Result;

use crate::install::UserConfig;

pub async fn ensure_authenticated() -> Result<UserConfig> {
    anyhow::bail!("authentication is not implemented yet")
}

