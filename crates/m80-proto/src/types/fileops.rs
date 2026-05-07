//! File operation payloads carried over the host↔guest wire.

/// Default raw byte cap for [`FileReadRequest`].
pub const FILE_READ_LIMIT_DEFAULT: u64 = 16 * 1024 * 1024;

/// Wire `kind` for [`FileReadRequest`].
pub const PAYLOAD_KIND_FILE_READ_REQUEST: &str = "file_read_request";
/// Wire `kind` for [`FileReadResponse`].
pub const PAYLOAD_KIND_FILE_READ_RESPONSE: &str = "file_read_response";
/// Wire `kind` for [`FileReadChunk`].
pub const PAYLOAD_KIND_FILE_READ_CHUNK: &str = "file_read_chunk";
/// Wire `kind` for [`FileWriteRequest`].
pub const PAYLOAD_KIND_FILE_WRITE_REQUEST: &str = "file_write_request";
/// Wire `kind` for [`FileWriteResponse`].
pub const PAYLOAD_KIND_FILE_WRITE_RESPONSE: &str = "file_write_response";
/// Wire `kind` for [`FileListRequest`].
pub const PAYLOAD_KIND_FILE_LIST_REQUEST: &str = "file_list_request";
/// Wire `kind` for [`FileListResponse`].
pub const PAYLOAD_KIND_FILE_LIST_RESPONSE: &str = "file_list_response";
/// Wire `kind` for [`FileStatRequest`].
pub const PAYLOAD_KIND_FILE_STAT_REQUEST: &str = "file_stat_request";
/// Wire `kind` for [`FileStatResponse`].
pub const PAYLOAD_KIND_FILE_STAT_RESPONSE: &str = "file_stat_response";
/// Wire `kind` for [`FileRemoveRequest`].
pub const PAYLOAD_KIND_FILE_REMOVE_REQUEST: &str = "file_remove_request";
/// Wire `kind` for [`FileRemoveResponse`].
pub const PAYLOAD_KIND_FILE_REMOVE_RESPONSE: &str = "file_remove_response";
/// Wire `kind` for [`FileMkdirRequest`].
pub const PAYLOAD_KIND_FILE_MKDIR_REQUEST: &str = "file_mkdir_request";
/// Wire `kind` for [`FileMkdirResponse`].
pub const PAYLOAD_KIND_FILE_MKDIR_RESPONSE: &str = "file_mkdir_response";
/// Wire `kind` for [`FileWriteBeginRequest`].
pub const PAYLOAD_KIND_FILE_WRITE_BEGIN_REQUEST: &str = "file_write_begin_request";
/// Wire `kind` for [`FileWriteBeginResponse`].
pub const PAYLOAD_KIND_FILE_WRITE_BEGIN_RESPONSE: &str = "file_write_begin_response";
/// Wire `kind` for [`FileWriteChunkRequest`].
pub const PAYLOAD_KIND_FILE_WRITE_CHUNK_REQUEST: &str = "file_write_chunk_request";
/// Wire `kind` for [`FileWriteChunkResponse`].
pub const PAYLOAD_KIND_FILE_WRITE_CHUNK_RESPONSE: &str = "file_write_chunk_response";
/// Wire `kind` for [`FileWriteCommitRequest`].
pub const PAYLOAD_KIND_FILE_WRITE_COMMIT_REQUEST: &str = "file_write_commit_request";
/// Wire `kind` for [`FileWriteCommitResponse`].
pub const PAYLOAD_KIND_FILE_WRITE_COMMIT_RESPONSE: &str = "file_write_commit_response";

/// Bounded file-operation failure vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileError {
    /// The target path does not exist.
    NotFound,
    /// The guest kernel denied the operation.
    PermissionDenied,
    /// A file was required but the target is a directory.
    IsADirectory,
    /// A directory was required but the target is not a directory.
    NotADirectory,
    /// The final path component is a symlink and the verb rejects symlinks.
    SymlinkRejected,
    /// The request exceeded the verb's size cap.
    TooLarge,
    /// Chunked upload frame did not match the next expected sequence.
    InvalidSequence,
    /// I/O failure that does not fit a narrower variant.
    Io,
}

/// File kind reported by list/stat operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Regular file.
    File,
    /// Directory.
    Directory,
    /// Symlink.
    Symlink,
    /// Other kernel file type.
    Other,
}

/// Directory entry returned by [`FileListResponse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Entry basename, not a full path.
    pub name: String,
    /// Entry kind.
    pub kind: FileKind,
    /// Entry size in bytes from symlink metadata.
    pub size: u64,
}

/// File metadata returned by [`FileStatResponse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    /// File kind.
    pub kind: FileKind,
    /// File size in bytes.
    pub size: u64,
    /// Modification timestamp as Unix milliseconds.
    pub mtime_unix_ms: i64,
    /// Unix mode bits as reported by metadata.
    pub mode: u32,
}

/// Read a file directly in the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReadRequest {
    /// Guest path to read.
    pub path: String,
    /// Maximum raw bytes to return. `None` uses [`FILE_READ_LIMIT_DEFAULT`].
    pub max_bytes: Option<u64>,
}

/// Response to [`FileReadRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReadResponse {
    /// Raw bytes read.
    pub bytes: Vec<u8>,
    /// True when bytes were capped by `max_bytes`.
    pub truncated: bool,
    /// Error discriminant; `None` on success.
    pub error: Option<FileError>,
}

/// Write a complete file in one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWriteRequest {
    /// Guest path to write. Parent directory must already exist.
    pub path: String,
    /// Raw file contents.
    pub bytes: Vec<u8>,
    /// Optional Unix mode applied after write.
    pub mode: Option<u32>,
}

/// Response to [`FileWriteRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWriteResponse {
    /// Number of raw bytes written.
    pub bytes_written: u64,
    /// Error discriminant; `None` on success.
    pub error: Option<FileError>,
}

/// List one directory level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileListRequest {
    /// Guest directory path.
    pub path: String,
}

/// Response to [`FileListRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileListResponse {
    /// Entries in one directory level.
    pub entries: Vec<DirEntry>,
    /// Error discriminant; `None` on success.
    pub error: Option<FileError>,
}

/// Stat one path without following the final symlink component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatRequest {
    /// Guest path to stat.
    pub path: String,
}

/// Response to [`FileStatRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatResponse {
    /// Metadata on success.
    pub stat: Option<FileStat>,
    /// Error discriminant; `None` on success.
    pub error: Option<FileError>,
}

/// Remove one non-directory path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRemoveRequest {
    /// Guest path to remove.
    pub path: String,
}

/// Response to [`FileRemoveRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRemoveResponse {
    /// True when a path was removed.
    pub removed: bool,
    /// Error discriminant; `None` on success.
    pub error: Option<FileError>,
}

/// Create a guest directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMkdirRequest {
    /// Guest directory path to create.
    pub path: String,
    /// Optional Unix mode applied after creation.
    pub mode: Option<u32>,
    /// Create missing parents when true.
    pub recursive: bool,
}

/// Response to [`FileMkdirRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMkdirResponse {
    /// True when this request created at least the final directory.
    pub created: bool,
    /// Error discriminant; `None` on success.
    pub error: Option<FileError>,
}

/// Begin a chunked upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWriteBeginRequest {
    /// Final guest path to commit.
    pub path: String,
    /// Optional Unix mode applied after commit.
    pub mode: Option<u32>,
}

/// Response to [`FileWriteBeginRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWriteBeginResponse {
    /// Per-connection upload id. `None` on error.
    pub upload_id: Option<String>,
    /// Error discriminant; `None` on success.
    pub error: Option<FileError>,
}

/// One chunk in an upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWriteChunkRequest {
    /// Upload id returned by begin.
    pub upload_id: String,
    /// Caller sequence number echoed in the response.
    pub seq: u64,
    /// Raw chunk bytes.
    pub bytes: Vec<u8>,
}

/// Response to [`FileWriteChunkRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWriteChunkResponse {
    /// Echoed upload id.
    pub upload_id: String,
    /// Echoed sequence number.
    pub seq: u64,
    /// Raw bytes accepted for this chunk.
    pub bytes_written: u64,
    /// Error discriminant; `None` on success.
    pub error: Option<FileError>,
}

/// Commit a chunked upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWriteCommitRequest {
    /// Upload id returned by begin.
    pub upload_id: String,
}

/// Response to [`FileWriteCommitRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileWriteCommitResponse {
    /// Total bytes written for the upload.
    pub bytes_written: u64,
    /// Error discriminant; `None` on success.
    pub error: Option<FileError>,
}

/// One chunk in a guest-to-host file read stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReadChunk {
    /// Caller sequence number.
    pub seq: u64,
    /// Raw bytes carried by this chunk.
    pub bytes: Vec<u8>,
    /// True on the terminal chunk for this read.
    pub done: bool,
    /// True when the read was capped by `max_bytes`.
    pub truncated: bool,
    /// Error discriminant; `None` on success.
    pub error: Option<FileError>,
}
