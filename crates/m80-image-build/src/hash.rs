//! sha256 helpers for build artifact hashing.

use std::io::Read;
use std::path::Path;

use anyhow::Context;
use sha2::{Digest, Sha256};

/// Compute the sha256 hex digest of a file at `path`.
///
/// Streams the file through `Sha256` in 64 KiB chunks so hashing a
/// multi-GiB rootfs doesn't allocate the whole file on the heap.
pub(crate) fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("opening {} for sha256", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("reading {} for sha256", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}
