use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use m80_proto::{
    DirEntry, FileError, FileKind, FileListRequest, FileListResponse, FileMkdirRequest,
    FileMkdirResponse, FileRemoveRequest, FileRemoveResponse, FileStat, FileStatRequest,
    FileStatResponse,
};

use super::map_io_error;

pub(super) fn list_dir(req: FileListRequest) -> FileListResponse {
    let dir = Path::new(&req.path);
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            return FileListResponse {
                entries: Vec::new(),
                error: Some(map_io_error(&e)),
            };
        }
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                return FileListResponse {
                    entries: Vec::new(),
                    error: Some(map_io_error(&e)),
                };
            }
        };
        let meta = match std::fs::symlink_metadata(entry.path()) {
            Ok(meta) => meta,
            Err(e) => {
                return FileListResponse {
                    entries: Vec::new(),
                    error: Some(map_io_error(&e)),
                };
            }
        };
        out.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            kind: kind_from_metadata(&meta),
            size: meta.len(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    FileListResponse {
        entries: out,
        error: None,
    }
}

pub(super) fn stat_file(req: FileStatRequest) -> FileStatResponse {
    match std::fs::symlink_metadata(&req.path) {
        Ok(meta) => FileStatResponse {
            stat: Some(stat_from_metadata(&meta)),
            error: None,
        },
        Err(e) => FileStatResponse {
            stat: None,
            error: Some(map_io_error(&e)),
        },
    }
}

pub(super) fn remove_file(req: FileRemoveRequest) -> FileRemoveResponse {
    match std::fs::symlink_metadata(&req.path) {
        Ok(meta) if meta.is_dir() => FileRemoveResponse {
            removed: false,
            error: Some(FileError::IsADirectory),
        },
        Ok(_) => match std::fs::remove_file(&req.path) {
            Ok(()) => FileRemoveResponse {
                removed: true,
                error: None,
            },
            Err(e) => FileRemoveResponse {
                removed: false,
                error: Some(map_io_error(&e)),
            },
        },
        Err(e) => FileRemoveResponse {
            removed: false,
            error: Some(map_io_error(&e)),
        },
    }
}

pub(super) fn mkdir(req: FileMkdirRequest) -> FileMkdirResponse {
    let path = Path::new(&req.path);
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => FileMkdirResponse {
            created: false,
            error: Some(FileError::SymlinkRejected),
        },
        Ok(meta) if meta.is_dir() => match apply_mode(path, req.mode) {
            Ok(()) => FileMkdirResponse {
                created: false,
                error: None,
            },
            Err(e) => FileMkdirResponse {
                created: false,
                error: Some(map_io_error(&e)),
            },
        },
        Ok(_) => FileMkdirResponse {
            created: false,
            error: Some(FileError::NotADirectory),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let created = if req.recursive {
                std::fs::create_dir_all(path)
            } else {
                std::fs::create_dir(path)
            };
            match created.and_then(|()| apply_mode(path, req.mode)) {
                Ok(()) => FileMkdirResponse {
                    created: true,
                    error: None,
                },
                Err(e) => FileMkdirResponse {
                    created: false,
                    error: Some(map_io_error(&e)),
                },
            }
        }
        Err(e) => FileMkdirResponse {
            created: false,
            error: Some(map_io_error(&e)),
        },
    }
}

fn apply_mode(path: &Path, mode: Option<u32>) -> std::io::Result<()> {
    if let Some(mode) = mode {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

fn stat_from_metadata(meta: &std::fs::Metadata) -> FileStat {
    FileStat {
        kind: kind_from_metadata(meta),
        size: meta.len(),
        mtime_unix_ms: meta.mtime().saturating_mul(1000) + meta.mtime_nsec() / 1_000_000,
        mode: meta.mode(),
    }
}

fn kind_from_metadata(meta: &std::fs::Metadata) -> FileKind {
    let ft = meta.file_type();
    if ft.is_symlink() {
        FileKind::Symlink
    } else if ft.is_file() {
        FileKind::File
    } else if ft.is_dir() {
        FileKind::Directory
    } else {
        FileKind::Other
    }
}
