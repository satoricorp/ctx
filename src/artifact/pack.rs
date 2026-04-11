use anyhow::Result;
use std::fs::File;
use std::path::Path;

pub fn pack_context(ctx_path: &Path, output_path: &Path) -> Result<()> {
    let tar_file = File::create(output_path)?;
    let encoder = zstd::Encoder::new(tar_file, 3)?;
    let mut archive = tar::Builder::new(encoder.auto_finish());
    let root_name = ctx_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("context.ctx");
    archive.append_dir_all(root_name, ctx_path)?;
    archive.finish()?;
    Ok(())
}

pub fn unpack_context(archive_path: &Path, destination_root: &Path) -> Result<()> {
    let archive_file = File::open(archive_path)?;
    let decoder = zstd::Decoder::new(archive_file)?;
    let mut archive = tar::Archive::new(decoder);
    archive.unpack(destination_root)?;
    Ok(())
}

