use clap::Args;

/// Flags shared by commands that resolve a named context under [`crate::artifact::context_path`].
#[derive(Debug, Args)]
pub struct ContextSelectArgs {
    #[arg(short = 'c', long = "context")]
    pub context: Option<String>,
    /// Scope context data to a deployment image tag or digest (same as env **CTX_IMAGE**).
    /// Contexts are stored under `…/contexts/images/<tag>/`. When set here, overrides **CTX_IMAGE** for this process.
    #[arg(long = "image", value_name = "TAG_OR_DIGEST")]
    pub image: Option<String>,
}

/// Apply [`ContextSelectArgs::image`] so [`crate::artifact::context_root`] includes the image subdirectory.
pub fn apply_context_image_flag(image: &Option<String>) {
    match image {
        Some(tag) if !tag.trim().is_empty() => {
            std::env::set_var("CTX_IMAGE", tag.trim());
        }
        Some(_) => {
            std::env::remove_var("CTX_IMAGE");
        }
        None => {}
    }
}
