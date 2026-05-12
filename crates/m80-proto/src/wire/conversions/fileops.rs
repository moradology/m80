use crate::types::{
    DirEntry, FileListRequest, FileListResponse, FileMkdirRequest, FileMkdirResponse,
    FileReadChunk, FileReadRequest, FileReadResponse, FileRemoveRequest, FileRemoveResponse,
    FileStat, FileStatRequest, FileStatResponse, FileWriteBeginRequest, FileWriteBeginResponse,
    FileWriteChunkRequest, FileWriteChunkResponse, FileWriteCommitRequest, FileWriteCommitResponse,
    FileWriteRequest, FileWriteResponse, PAYLOAD_KIND_FILE_LIST_REQUEST,
    PAYLOAD_KIND_FILE_LIST_RESPONSE, PAYLOAD_KIND_FILE_MKDIR_REQUEST,
    PAYLOAD_KIND_FILE_MKDIR_RESPONSE, PAYLOAD_KIND_FILE_READ_CHUNK, PAYLOAD_KIND_FILE_READ_REQUEST,
    PAYLOAD_KIND_FILE_READ_RESPONSE, PAYLOAD_KIND_FILE_REMOVE_REQUEST,
    PAYLOAD_KIND_FILE_REMOVE_RESPONSE, PAYLOAD_KIND_FILE_STAT_REQUEST,
    PAYLOAD_KIND_FILE_STAT_RESPONSE, PAYLOAD_KIND_FILE_WRITE_BEGIN_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_BEGIN_RESPONSE, PAYLOAD_KIND_FILE_WRITE_CHUNK_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_CHUNK_RESPONSE, PAYLOAD_KIND_FILE_WRITE_COMMIT_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_COMMIT_RESPONSE, PAYLOAD_KIND_FILE_WRITE_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_RESPONSE,
};

use super::{Payload, ProtoError, WireDirEntry, WireFileListRequest, WireFileListResponse, WireFileMkdirRequest, WireFileMkdirResponse, WireFileReadChunk, WireFileReadRequest, WireFileReadResponse, WireFileRemoveRequest, WireFileRemoveResponse, WireFileStat, WireFileStatRequest, WireFileStatResponse, WireFileWriteBeginRequest, WireFileWriteBeginResponse, WireFileWriteChunkRequest, WireFileWriteChunkResponse, WireFileWriteCommitRequest, WireFileWriteCommitResponse, WireFileWriteRequest, WireFileWriteResponse, WirePayload, file_kind_from_i32, file_kind_to_i32, opt_file_error_from_i32, opt_file_error_to_i32, payload_name};

payload_impl!(
    FileReadRequest,
    PAYLOAD_KIND_FILE_READ_REQUEST,
    FileReadRequest,
    WireFileReadRequest,
    |v| WireFileReadRequest {
        path: v.path,
        max_bytes: v.max_bytes,
    },
    |v| Ok(FileReadRequest {
        path: v.path,
        max_bytes: v.max_bytes,
    })
);

payload_impl!(
    FileReadResponse,
    PAYLOAD_KIND_FILE_READ_RESPONSE,
    FileReadResponse,
    WireFileReadResponse,
    |v| WireFileReadResponse {
        bytes: v.bytes,
        truncated: v.truncated,
        error: opt_file_error_to_i32(v.error),
    },
    |v| Ok(FileReadResponse {
        bytes: v.bytes,
        truncated: v.truncated,
        error: opt_file_error_from_i32(v.error)?,
    })
);

payload_impl!(
    FileReadChunk,
    PAYLOAD_KIND_FILE_READ_CHUNK,
    FileReadChunk,
    WireFileReadChunk,
    |v| WireFileReadChunk {
        seq: v.seq,
        bytes: v.bytes,
        done: v.done,
        truncated: v.truncated,
        error: opt_file_error_to_i32(v.error),
    },
    |v| Ok(FileReadChunk {
        seq: v.seq,
        bytes: v.bytes,
        done: v.done,
        truncated: v.truncated,
        error: opt_file_error_from_i32(v.error)?,
    })
);

payload_impl!(
    FileWriteRequest,
    PAYLOAD_KIND_FILE_WRITE_REQUEST,
    FileWriteRequest,
    WireFileWriteRequest,
    |v| WireFileWriteRequest {
        path: v.path,
        bytes: v.bytes,
        mode: v.mode,
    },
    |v| Ok(FileWriteRequest {
        path: v.path,
        bytes: v.bytes,
        mode: v.mode,
    })
);

payload_impl!(
    FileWriteResponse,
    PAYLOAD_KIND_FILE_WRITE_RESPONSE,
    FileWriteResponse,
    WireFileWriteResponse,
    |v| WireFileWriteResponse {
        bytes_written: v.bytes_written,
        error: opt_file_error_to_i32(v.error),
    },
    |v| Ok(FileWriteResponse {
        bytes_written: v.bytes_written,
        error: opt_file_error_from_i32(v.error)?,
    })
);

payload_impl!(
    FileListRequest,
    PAYLOAD_KIND_FILE_LIST_REQUEST,
    FileListRequest,
    WireFileListRequest,
    |v| WireFileListRequest { path: v.path },
    |v| Ok(FileListRequest { path: v.path })
);

payload_impl!(
    FileListResponse,
    PAYLOAD_KIND_FILE_LIST_RESPONSE,
    FileListResponse,
    WireFileListResponse,
    |v| WireFileListResponse {
        entries: v
            .entries
            .into_iter()
            .map(|e| WireDirEntry {
                name: e.name,
                kind: file_kind_to_i32(e.kind),
                size: e.size,
            })
            .collect(),
        error: opt_file_error_to_i32(v.error),
    },
    |v| Ok(FileListResponse {
        entries: v
            .entries
            .into_iter()
            .map(|e| {
                Ok(DirEntry {
                    name: e.name,
                    kind: file_kind_from_i32(e.kind)?,
                    size: e.size,
                })
            })
            .collect::<Result<Vec<_>, ProtoError>>()?,
        error: opt_file_error_from_i32(v.error)?,
    })
);

payload_impl!(
    FileStatRequest,
    PAYLOAD_KIND_FILE_STAT_REQUEST,
    FileStatRequest,
    WireFileStatRequest,
    |v| WireFileStatRequest { path: v.path },
    |v| Ok(FileStatRequest { path: v.path })
);

payload_impl!(
    FileStatResponse,
    PAYLOAD_KIND_FILE_STAT_RESPONSE,
    FileStatResponse,
    WireFileStatResponse,
    |v| WireFileStatResponse {
        stat: v.stat.map(|s| WireFileStat {
            kind: file_kind_to_i32(s.kind),
            size: s.size,
            mtime_unix_ms: s.mtime_unix_ms,
            mode: s.mode,
        }),
        error: opt_file_error_to_i32(v.error),
    },
    |v| Ok(FileStatResponse {
        stat: v
            .stat
            .map(|s| {
                Ok::<FileStat, ProtoError>(FileStat {
                    kind: file_kind_from_i32(s.kind)?,
                    size: s.size,
                    mtime_unix_ms: s.mtime_unix_ms,
                    mode: s.mode,
                })
            })
            .transpose()?,
        error: opt_file_error_from_i32(v.error)?,
    })
);

payload_impl!(
    FileRemoveRequest,
    PAYLOAD_KIND_FILE_REMOVE_REQUEST,
    FileRemoveRequest,
    WireFileRemoveRequest,
    |v| WireFileRemoveRequest { path: v.path },
    |v| Ok(FileRemoveRequest { path: v.path })
);

payload_impl!(
    FileRemoveResponse,
    PAYLOAD_KIND_FILE_REMOVE_RESPONSE,
    FileRemoveResponse,
    WireFileRemoveResponse,
    |v| WireFileRemoveResponse {
        removed: v.removed,
        error: opt_file_error_to_i32(v.error),
    },
    |v| Ok(FileRemoveResponse {
        removed: v.removed,
        error: opt_file_error_from_i32(v.error)?,
    })
);

payload_impl!(
    FileMkdirRequest,
    PAYLOAD_KIND_FILE_MKDIR_REQUEST,
    FileMkdirRequest,
    WireFileMkdirRequest,
    |v| WireFileMkdirRequest {
        path: v.path,
        mode: v.mode,
        recursive: v.recursive,
    },
    |v| Ok(FileMkdirRequest {
        path: v.path,
        mode: v.mode,
        recursive: v.recursive,
    })
);

payload_impl!(
    FileMkdirResponse,
    PAYLOAD_KIND_FILE_MKDIR_RESPONSE,
    FileMkdirResponse,
    WireFileMkdirResponse,
    |v| WireFileMkdirResponse {
        created: v.created,
        error: opt_file_error_to_i32(v.error),
    },
    |v| Ok(FileMkdirResponse {
        created: v.created,
        error: opt_file_error_from_i32(v.error)?,
    })
);

payload_impl!(
    FileWriteBeginRequest,
    PAYLOAD_KIND_FILE_WRITE_BEGIN_REQUEST,
    FileWriteBeginRequest,
    WireFileWriteBeginRequest,
    |v| WireFileWriteBeginRequest {
        path: v.path,
        mode: v.mode,
    },
    |v| Ok(FileWriteBeginRequest {
        path: v.path,
        mode: v.mode,
    })
);

payload_impl!(
    FileWriteBeginResponse,
    PAYLOAD_KIND_FILE_WRITE_BEGIN_RESPONSE,
    FileWriteBeginResponse,
    WireFileWriteBeginResponse,
    |v| WireFileWriteBeginResponse {
        upload_id: v.upload_id,
        error: opt_file_error_to_i32(v.error),
    },
    |v| Ok(FileWriteBeginResponse {
        upload_id: v.upload_id,
        error: opt_file_error_from_i32(v.error)?,
    })
);

payload_impl!(
    FileWriteChunkRequest,
    PAYLOAD_KIND_FILE_WRITE_CHUNK_REQUEST,
    FileWriteChunkRequest,
    WireFileWriteChunkRequest,
    |v| WireFileWriteChunkRequest {
        upload_id: v.upload_id,
        seq: v.seq,
        bytes: v.bytes,
    },
    |v| Ok(FileWriteChunkRequest {
        upload_id: v.upload_id,
        seq: v.seq,
        bytes: v.bytes,
    })
);

payload_impl!(
    FileWriteChunkResponse,
    PAYLOAD_KIND_FILE_WRITE_CHUNK_RESPONSE,
    FileWriteChunkResponse,
    WireFileWriteChunkResponse,
    |v| WireFileWriteChunkResponse {
        upload_id: v.upload_id,
        seq: v.seq,
        bytes_written: v.bytes_written,
        error: opt_file_error_to_i32(v.error),
    },
    |v| Ok(FileWriteChunkResponse {
        upload_id: v.upload_id,
        seq: v.seq,
        bytes_written: v.bytes_written,
        error: opt_file_error_from_i32(v.error)?,
    })
);

payload_impl!(
    FileWriteCommitRequest,
    PAYLOAD_KIND_FILE_WRITE_COMMIT_REQUEST,
    FileWriteCommitRequest,
    WireFileWriteCommitRequest,
    |v| WireFileWriteCommitRequest {
        upload_id: v.upload_id,
    },
    |v| Ok(FileWriteCommitRequest {
        upload_id: v.upload_id,
    })
);

payload_impl!(
    FileWriteCommitResponse,
    PAYLOAD_KIND_FILE_WRITE_COMMIT_RESPONSE,
    FileWriteCommitResponse,
    WireFileWriteCommitResponse,
    |v| WireFileWriteCommitResponse {
        bytes_written: v.bytes_written,
        error: opt_file_error_to_i32(v.error),
    },
    |v| Ok(FileWriteCommitResponse {
        bytes_written: v.bytes_written,
        error: opt_file_error_from_i32(v.error)?,
    })
);
