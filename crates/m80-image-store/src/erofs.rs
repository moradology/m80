//! Erofs compatibility probes for imported artifacts.

use std::path::Path;
use std::process::Command;

use crate::StoreError;

const EROFS_MAGIC: &str = "0xE0F5E1E2";
const SUPPORTED_FEATURES: &[&str] = &["sb_csum", "mtime", "0padding"];
const SUPPORTED_COMPRESSION_ALGS: &[&str] = &["lz4", "lz4hc"];

pub(crate) fn validate_supported_erofs(path: &Path) -> Result<(), StoreError> {
    let output = Command::new("dump.erofs")
        .arg("-s")
        .arg(path)
        .output()
        .map_err(|err| StoreError::ErofsProbeFailed {
            path: path.to_path_buf(),
            detail: err.to_string(),
        })?;
    if !output.status.success() {
        return Err(StoreError::ErofsProbeFailed {
            path: path.to_path_buf(),
            detail: command_failure_detail(output),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    validate_dump_erofs_superblock(path, &stdout)
}

fn validate_dump_erofs_superblock(path: &Path, raw: &str) -> Result<(), StoreError> {
    let mut saw_magic = false;
    let mut saw_features = false;
    for line in raw.lines() {
        if let Some(value) = field_value(line, "Filesystem magic number:") {
            saw_magic = true;
            if value != EROFS_MAGIC {
                return Err(StoreError::ErofsProbeFailed {
                    path: path.to_path_buf(),
                    detail: format!("unexpected erofs magic {value}"),
                });
            }
        } else if let Some(value) = field_value(line, "Filesystem compr_algs:") {
            for algorithm in split_tokens(value) {
                if !SUPPORTED_COMPRESSION_ALGS.contains(&algorithm) {
                    return Err(StoreError::UnsupportedErofsCompression {
                        path: path.to_path_buf(),
                        algorithm: algorithm.to_owned(),
                    });
                }
            }
        } else if let Some(value) = field_value(line, "Filesystem features:") {
            saw_features = true;
            for feature in split_tokens(value) {
                if !SUPPORTED_FEATURES.contains(&feature) {
                    return Err(StoreError::UnsupportedErofsFeature {
                        path: path.to_path_buf(),
                        feature: feature.to_owned(),
                    });
                }
            }
        }
    }
    if !saw_magic {
        return Err(StoreError::ErofsProbeFailed {
            path: path.to_path_buf(),
            detail: "dump.erofs -s output did not include filesystem magic".to_owned(),
        });
    }
    if !saw_features {
        return Err(StoreError::ErofsProbeFailed {
            path: path.to_path_buf(),
            detail: "dump.erofs -s output did not include feature list".to_owned(),
        });
    }
    Ok(())
}

fn field_value<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.strip_prefix(prefix).map(str::trim)
}

fn split_tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|ch: char| ch.is_ascii_whitespace() || ch == ',')
        .filter(|token| !token.is_empty())
}

fn command_failure_detail(output: std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !stdout.is_empty() {
        return stdout;
    }
    format!("dump.erofs exited with {}", output.status)
}

#[cfg(test)]
mod tests {
    use super::validate_dump_erofs_superblock;
    use crate::StoreError;
    use std::path::Path;

    #[test]
    fn accepts_pinned_erofs_feature_floor() {
        validate_dump_erofs_superblock(
            Path::new("/image.erofs"),
            "\
Filesystem magic number:                      0xE0F5E1E2
Filesystem lz4_max_distance:                  65535
Filesystem features:                          sb_csum mtime 0padding
",
        )
        .unwrap();
    }

    #[test]
    fn rejects_unknown_erofs_feature() {
        let err = validate_dump_erofs_superblock(
            Path::new("/image.erofs"),
            "\
Filesystem magic number:                      0xE0F5E1E2
Filesystem features:                          sb_csum chunked_file
",
        )
        .unwrap_err();

        assert!(
            matches!(
                err,
                StoreError::UnsupportedErofsFeature { ref feature, .. }
                    if feature == "chunked_file"
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_unpinned_erofs_compressor() {
        let err = validate_dump_erofs_superblock(
            Path::new("/image.erofs"),
            "\
Filesystem magic number:                      0xE0F5E1E2
Filesystem compr_algs:                        zstd
Filesystem features:                          sb_csum mtime
",
        )
        .unwrap_err();

        assert!(
            matches!(
                err,
                StoreError::UnsupportedErofsCompression { ref algorithm, .. }
                    if algorithm == "zstd"
            ),
            "got {err:?}"
        );
    }
}
