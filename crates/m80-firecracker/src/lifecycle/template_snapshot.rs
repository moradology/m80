//! Template snapshot path admission.

use std::path::{Path, PathBuf};

use m80_snapshot::SnapshotPaths;

use crate::error::{ConfigError, FcError};
use crate::lifecycle::{PreparedSnapshotPaths, SNAPSHOT_BIND_DEST};

pub(crate) fn prepare_template_snapshot_paths(
    paths: &SnapshotPaths,
) -> Result<PreparedSnapshotPaths, FcError> {
    let host_parent = paths.vm_state.parent().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "template.snapshot.vm_state",
            reason: "path must have a parent directory".into(),
        })
    })?;
    let mem_parent = paths.mem.parent().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "template.snapshot.mem",
            reason: "path must have a parent directory".into(),
        })
    })?;
    if host_parent != mem_parent {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "template.snapshot",
            reason: "vm_state and mem paths must live in the same directory".into(),
        }));
    }
    let vm_name = paths.vm_state.file_name().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "template.snapshot.vm_state",
            reason: "path must have a file name".into(),
        })
    })?;
    let mem_name = paths.mem.file_name().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "template.snapshot.mem",
            reason: "path must have a file name".into(),
        })
    })?;

    reject_symlink_component(&paths.vm_state, "template.snapshot.vm_state")?;
    reject_symlink_component(&paths.mem, "template.snapshot.mem")?;
    let host_parent = std::fs::canonicalize(host_parent).map_err(|source| FcError::PathIo {
        path: host_parent.to_path_buf(),
        source,
    })?;
    let in_jail_parent = PathBuf::from("/").join(SNAPSHOT_BIND_DEST);

    Ok(PreparedSnapshotPaths {
        host_parent,
        jail_paths: SnapshotPaths {
            vm_state: in_jail_parent.join(vm_name),
            mem: in_jail_parent.join(mem_name),
        },
    })
}

fn reject_symlink_component(path: &Path, field: &'static str) -> Result<(), FcError> {
    if !path.is_absolute() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field,
            reason: "path must be absolute".into(),
        }));
    }
    let mut cursor = PathBuf::new();
    for component in path.components() {
        cursor.push(component.as_os_str());
        let metadata = std::fs::symlink_metadata(&cursor).map_err(|source| FcError::PathIo {
            path: cursor.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field,
                reason: format!("path component {} is a symlink", cursor.display()),
            }));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_snapshot_paths_accept_store_dir_outside_run_root() {
        let store_dir = tempfile::tempdir().expect("store");
        let paths = write_template_snapshot_pair(store_dir.path());

        let prepared = prepare_template_snapshot_paths(&paths).expect("template paths");

        assert_eq!(
            prepared.host_parent,
            store_dir
                .path()
                .canonicalize()
                .expect("canonical store dir")
        );
        assert_eq!(prepared.jail_paths.vm_state, Path::new("/snapshot/vm.snap"));
        assert_eq!(prepared.jail_paths.mem, Path::new("/snapshot/mem.snap"));
    }

    #[test]
    fn template_snapshot_paths_reject_symlinked_store_parent() {
        let real_store = tempfile::tempdir().expect("real store");
        let link_parent = tempfile::tempdir().expect("link parent");
        let link = link_parent.path().join("store-link");
        std::os::unix::fs::symlink(real_store.path(), &link).expect("symlink");
        let paths = write_template_snapshot_pair(&link);

        let err = prepare_template_snapshot_paths(&paths)
            .expect_err("symlinked template store path must fail closed");

        assert!(
            matches!(
                err,
                FcError::Config(ConfigError::InvalidValue {
                    field: "template.snapshot.vm_state",
                    ..
                })
            ),
            "expected template symlink rejection, got {err:?}"
        );
    }

    fn write_template_snapshot_pair(parent: &Path) -> SnapshotPaths {
        std::fs::create_dir_all(parent).expect("template snapshot dir");
        let paths = SnapshotPaths {
            vm_state: parent.join("vm.snap"),
            mem: parent.join("mem.snap"),
        };
        std::fs::write(&paths.vm_state, b"vm").expect("write vm");
        std::fs::write(&paths.mem, b"mem").expect("write mem");
        paths
    }
}
