use std::fs;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use m80_firecracker::{ConfigError, FcError};
use m80_observability::{aggregate_health, probe, render_prometheus};

use crate::args::MetricsArgs;
use crate::config;
use crate::errors;

use super::warm;

const TEXTFILE_TMP_MODE: u32 = 0o644;

pub(super) fn cmd_metrics(args: MetricsArgs, json: bool) -> anyhow::Result<i32> {
    if json {
        let err = FcError::Config(ConfigError::InvalidValue {
            field: "json",
            reason: "m80 metrics emits Prometheus exposition text; omit --json".to_owned(),
        });
        return Ok(errors::render_error(&err, true));
    }

    let run_root = match config::resolve_run_root() {
        Ok(run_root) => run_root,
        Err(err) => return Ok(errors::render_error(&err, false)),
    };
    let records = probe(&run_root)?;
    let health = aggregate_health(&records);
    let mut metrics = m80_firecracker::ops_metrics_snapshot();
    metrics.vm_count = health.total;
    if let Some(snapshot) = warm::current_warm_pool_snapshot() {
        metrics.warm_pool = Some(m80_firecracker::warm_pool_metrics(snapshot));
    }
    let rendered = render_prometheus(&health, &metrics);

    if let Some(path) = args.textfile {
        write_textfile(&path, &rendered)?;
    } else {
        print!("{rendered}");
    }

    Ok(0)
}

fn write_textfile(path: &Path, rendered: &str) -> Result<(), FcError> {
    let tmp_path = textfile_tmp_path(path);
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(TEXTFILE_TMP_MODE)
        .open(&tmp_path)
        .map_err(|source| FcError::PathIo {
            path: tmp_path.clone(),
            source,
        })?;
    file.write_all(rendered.as_bytes())
        .map_err(|source| FcError::PathIo {
            path: tmp_path.clone(),
            source,
        })?;
    file.sync_all().map_err(|source| FcError::PathIo {
        path: tmp_path.clone(),
        source,
    })?;
    drop(file);
    fs::rename(&tmp_path, path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })
}

fn textfile_tmp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("m80.prom");
    let tmp_name = format!(".{file_name}.{}.tmp", std::process::id());
    path.with_file_name(tmp_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textfile_tmp_path_stays_next_to_target() {
        let path = Path::new("/tmp/node-exporter/m80.prom");
        assert_eq!(
            textfile_tmp_path(path),
            PathBuf::from(format!(
                "/tmp/node-exporter/.m80.prom.{}.tmp",
                std::process::id()
            ))
        );
    }

    #[test]
    fn write_textfile_replaces_target_when_parent_exists() {
        let temp = tempfile::tempdir().unwrap();
        let collector = temp.path().join("collector");
        fs::create_dir(&collector).unwrap();
        let path = collector.join("m80.prom");

        write_textfile(&path, "m80_launches_total 1\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "m80_launches_total 1\n");

        write_textfile(&path, "m80_launches_total 2\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "m80_launches_total 2\n");
    }

    #[test]
    fn write_textfile_requires_existing_parent() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing").join("m80.prom");

        let err = write_textfile(&path, "m80_launches_total 1\n").unwrap_err();

        assert!(matches!(err, FcError::PathIo { .. }));
        assert!(!path.exists());
    }
}
