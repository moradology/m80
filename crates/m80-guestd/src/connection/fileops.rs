use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::Context;
use m80_proto::{
    read_frame, write_frame, DirEntry, Envelope, FileError, FileKind, FileListRequest,
    FileListResponse, FileReadRequest, FileReadResponse, FileRemoveRequest, FileRemoveResponse,
    FileStat, FileStatRequest, FileStatResponse, FileWriteBeginRequest, FileWriteBeginResponse,
    FileWriteChunkRequest, FileWriteChunkResponse, FileWriteCommitRequest, FileWriteCommitResponse,
    FileWriteRequest, FileWriteResponse, FILE_READ_LIMIT_DEFAULT, PAYLOAD_KIND_FILE_LIST_REQUEST,
    PAYLOAD_KIND_FILE_READ_REQUEST, PAYLOAD_KIND_FILE_REMOVE_REQUEST,
    PAYLOAD_KIND_FILE_STAT_REQUEST, PAYLOAD_KIND_FILE_WRITE_BEGIN_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_CHUNK_REQUEST, PAYLOAD_KIND_FILE_WRITE_COMMIT_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_REQUEST,
};

use super::ConnectionOutcome;

const WRITE_ENVELOPE_CAP: usize = 64 * 1024 * 1024;

#[derive(Default)]
struct Uploads {
    next_id: u64,
    open: HashMap<String, Upload>,
}

struct Upload {
    final_path: PathBuf,
    temp_path: PathBuf,
    file: File,
    bytes_written: u64,
    mode: Option<u32>,
}

impl Drop for Uploads {
    fn drop(&mut self) {
        for (_id, upload) in self.open.drain() {
            let _ = std::fs::remove_file(upload.temp_path);
        }
    }
}

pub fn is_fileop_kind(kind: &str) -> bool {
    matches!(
        kind,
        PAYLOAD_KIND_FILE_READ_REQUEST
            | PAYLOAD_KIND_FILE_WRITE_REQUEST
            | PAYLOAD_KIND_FILE_LIST_REQUEST
            | PAYLOAD_KIND_FILE_STAT_REQUEST
            | PAYLOAD_KIND_FILE_REMOVE_REQUEST
            | PAYLOAD_KIND_FILE_WRITE_BEGIN_REQUEST
            | PAYLOAD_KIND_FILE_WRITE_CHUNK_REQUEST
            | PAYLOAD_KIND_FILE_WRITE_COMMIT_REQUEST
    )
}

pub fn handle_fileop<R, W>(
    first: Envelope<serde_json::Value>,
    mut reader: R,
    writer: &mut W,
) -> anyhow::Result<ConnectionOutcome>
where
    R: BufRead,
    W: Write,
{
    let mut uploads = Uploads::default();
    let keep_open = handle_one(first, writer, &mut uploads)?;
    if !keep_open {
        nix::unistd::sync();
        return Ok(ConnectionOutcome::Continue);
    }

    loop {
        let next: Envelope<serde_json::Value> = match read_frame(&mut reader) {
            Ok(frame) => frame,
            Err(_) => return Ok(ConnectionOutcome::Continue),
        };
        let keep_open = handle_one(next, writer, &mut uploads)?;
        if !keep_open {
            nix::unistd::sync();
            return Ok(ConnectionOutcome::Continue);
        }
    }
}

fn handle_one<W: Write>(
    raw: Envelope<serde_json::Value>,
    writer: &mut W,
    uploads: &mut Uploads,
) -> anyhow::Result<bool> {
    match raw.kind.as_str() {
        PAYLOAD_KIND_FILE_READ_REQUEST => {
            let req = decode::<FileReadRequest>(raw.payload)?;
            respond(writer, raw.request_id, read_file(req))?;
            Ok(false)
        }
        PAYLOAD_KIND_FILE_WRITE_REQUEST => {
            let req = decode::<FileWriteRequest>(raw.payload)?;
            respond(writer, raw.request_id, write_file(req))?;
            Ok(false)
        }
        PAYLOAD_KIND_FILE_LIST_REQUEST => {
            let req = decode::<FileListRequest>(raw.payload)?;
            respond(writer, raw.request_id, list_dir(req))?;
            Ok(false)
        }
        PAYLOAD_KIND_FILE_STAT_REQUEST => {
            let req = decode::<FileStatRequest>(raw.payload)?;
            respond(writer, raw.request_id, stat_file(req))?;
            Ok(false)
        }
        PAYLOAD_KIND_FILE_REMOVE_REQUEST => {
            let req = decode::<FileRemoveRequest>(raw.payload)?;
            respond(writer, raw.request_id, remove_file(req))?;
            Ok(false)
        }
        PAYLOAD_KIND_FILE_WRITE_BEGIN_REQUEST => {
            let req = decode::<FileWriteBeginRequest>(raw.payload)?;
            let response = begin_upload(req, uploads);
            let keep_open = response.error.is_none();
            respond(writer, raw.request_id, response)?;
            Ok(keep_open)
        }
        PAYLOAD_KIND_FILE_WRITE_CHUNK_REQUEST => {
            let req = decode::<FileWriteChunkRequest>(raw.payload)?;
            respond(writer, raw.request_id, write_chunk(req, uploads))?;
            Ok(true)
        }
        PAYLOAD_KIND_FILE_WRITE_COMMIT_REQUEST => {
            let req = decode::<FileWriteCommitRequest>(raw.payload)?;
            respond(writer, raw.request_id, commit_upload(req, uploads))?;
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> anyhow::Result<T> {
    serde_json::from_value(value).context("decode file operation payload")
}

fn respond<T: m80_proto::Payload + serde::Serialize, W: Write>(
    writer: &mut W,
    request_id: Option<String>,
    payload: T,
) -> anyhow::Result<()> {
    let out = match request_id {
        Some(id) => Envelope::with_request_id(payload, id),
        None => Envelope::new(payload),
    };
    write_frame(writer, &out)?;
    writer.flush()?;
    Ok(())
}

fn read_file(req: FileReadRequest) -> FileReadResponse {
    let path = Path::new(&req.path);
    let limit = req
        .max_bytes
        .unwrap_or(FILE_READ_LIMIT_DEFAULT)
        .min(usize::MAX as u64) as usize;
    match open_nofollow_read(path) {
        Ok(mut file) => {
            let mut bytes = Vec::new();
            let mut limited = (&mut file).take(limit as u64 + 1);
            match limited.read_to_end(&mut bytes) {
                Ok(_) => {
                    let truncated = bytes.len() > limit;
                    bytes.truncate(limit);
                    FileReadResponse {
                        bytes,
                        truncated,
                        error: None,
                    }
                }
                Err(e) => FileReadResponse {
                    bytes: Vec::new(),
                    truncated: false,
                    error: Some(map_io_error(&e)),
                },
            }
        }
        Err(error) => FileReadResponse {
            bytes: Vec::new(),
            truncated: false,
            error: Some(error),
        },
    }
}

fn write_file(req: FileWriteRequest) -> FileWriteResponse {
    if req.bytes.len() > WRITE_ENVELOPE_CAP {
        return FileWriteResponse {
            bytes_written: 0,
            error: Some(FileError::TooLarge),
        };
    }
    let path = Path::new(&req.path);
    match open_nofollow_write(path, false) {
        Ok(mut file) => match file.write_all(&req.bytes).and_then(|()| file.sync_all()) {
            Ok(()) => {
                if let Some(mode) = req.mode {
                    if let Err(e) =
                        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                    {
                        return FileWriteResponse {
                            bytes_written: 0,
                            error: Some(map_io_error(&e)),
                        };
                    }
                }
                FileWriteResponse {
                    bytes_written: req.bytes.len() as u64,
                    error: None,
                }
            }
            Err(e) => FileWriteResponse {
                bytes_written: 0,
                error: Some(map_io_error(&e)),
            },
        },
        Err(error) => FileWriteResponse {
            bytes_written: 0,
            error: Some(error),
        },
    }
}

fn list_dir(req: FileListRequest) -> FileListResponse {
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

fn stat_file(req: FileStatRequest) -> FileStatResponse {
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

fn remove_file(req: FileRemoveRequest) -> FileRemoveResponse {
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

fn begin_upload(req: FileWriteBeginRequest, uploads: &mut Uploads) -> FileWriteBeginResponse {
    let final_path = PathBuf::from(&req.path);
    if let Ok(meta) = std::fs::symlink_metadata(&final_path) {
        if meta.file_type().is_symlink() {
            return FileWriteBeginResponse {
                upload_id: None,
                error: Some(FileError::SymlinkRejected),
            };
        }
        if meta.is_dir() {
            return FileWriteBeginResponse {
                upload_id: None,
                error: Some(FileError::IsADirectory),
            };
        }
    }
    uploads.next_id = uploads.next_id.saturating_add(1);
    let upload_id = format!("u{}", uploads.next_id);
    let temp_path = PathBuf::from(format!("{}.m80-upload.{upload_id}", req.path));
    let file = match open_nofollow_write(&temp_path, true) {
        Ok(file) => file,
        Err(error) => {
            return FileWriteBeginResponse {
                upload_id: None,
                error: Some(error),
            };
        }
    };
    uploads.open.insert(
        upload_id.clone(),
        Upload {
            final_path,
            temp_path,
            file,
            bytes_written: 0,
            mode: req.mode,
        },
    );
    FileWriteBeginResponse {
        upload_id: Some(upload_id),
        error: None,
    }
}

fn write_chunk(req: FileWriteChunkRequest, uploads: &mut Uploads) -> FileWriteChunkResponse {
    let Some(upload) = uploads.open.get_mut(&req.upload_id) else {
        return FileWriteChunkResponse {
            upload_id: req.upload_id,
            seq: req.seq,
            bytes_written: 0,
            error: Some(FileError::NotFound),
        };
    };
    match upload.file.write_all(&req.bytes) {
        Ok(()) => {
            upload.bytes_written = upload.bytes_written.saturating_add(req.bytes.len() as u64);
            FileWriteChunkResponse {
                upload_id: req.upload_id,
                seq: req.seq,
                bytes_written: req.bytes.len() as u64,
                error: None,
            }
        }
        Err(e) => FileWriteChunkResponse {
            upload_id: req.upload_id,
            seq: req.seq,
            bytes_written: 0,
            error: Some(map_io_error(&e)),
        },
    }
}

fn commit_upload(req: FileWriteCommitRequest, uploads: &mut Uploads) -> FileWriteCommitResponse {
    let Some(upload) = uploads.open.remove(&req.upload_id) else {
        return FileWriteCommitResponse {
            bytes_written: 0,
            error: Some(FileError::NotFound),
        };
    };
    match commit_upload_inner(upload) {
        Ok(bytes_written) => FileWriteCommitResponse {
            bytes_written,
            error: None,
        },
        Err((temp_path, error)) => {
            let _ = std::fs::remove_file(temp_path);
            FileWriteCommitResponse {
                bytes_written: 0,
                error: Some(error),
            }
        }
    }
}

fn commit_upload_inner(upload: Upload) -> Result<u64, (PathBuf, FileError)> {
    if let Err(e) = upload.file.sync_all() {
        return Err((upload.temp_path, map_io_error(&e)));
    }
    drop(upload.file);
    if let Some(mode) = upload.mode {
        if let Err(e) =
            std::fs::set_permissions(&upload.temp_path, std::fs::Permissions::from_mode(mode))
        {
            return Err((upload.temp_path, map_io_error(&e)));
        }
    }
    if let Err(e) = std::fs::rename(&upload.temp_path, &upload.final_path) {
        return Err((upload.temp_path, map_io_error(&e)));
    }
    Ok(upload.bytes_written)
}

fn open_nofollow_read(path: &Path) -> Result<File, FileError> {
    match OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => Ok(file),
        Err(e) => Err(map_io_error(&e)),
    }
}

fn open_nofollow_write(path: &Path, create_new: bool) -> Result<File, FileError> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .mode(0o600);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    options.open(path).map_err(|e| map_io_error(&e))
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

fn map_io_error(e: &std::io::Error) -> FileError {
    match e.raw_os_error() {
        Some(code) if code == nix::libc::ELOOP => FileError::SymlinkRejected,
        Some(code) if code == nix::libc::EISDIR => FileError::IsADirectory,
        Some(code) if code == nix::libc::ENOTDIR => FileError::NotADirectory,
        _ => match e.kind() {
            std::io::ErrorKind::NotFound => FileError::NotFound,
            std::io::ErrorKind::PermissionDenied => FileError::PermissionDenied,
            _ => FileError::Io,
        },
    }
}
