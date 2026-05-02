//! sha256 helpers for build artifact hashing.

use std::path::Path;

use anyhow::Context;
use sha2::{Digest, Sha256};

/// Compute the sha256 hex digest of a file at `path`.
pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading {} for sha256", path.display()))?;
    Ok(hex::encode(Sha256::digest(&bytes)))
}
