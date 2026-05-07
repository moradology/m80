//! Public payload conversions for protobuf wire messages.

use crate::error::ProtoError;
use crate::types::{
    CancelResponse, CancelRequest, CancelStatus, DirEntry, ExecExit, ExecRequest, ExecResponse,
    ExecStatus, ExecStderr, ExecStdout, ExecTiming, FileError, FileKind, FileListRequest,
    FileListResponse, FileReadChunk, FileReadRequest, FileReadResponse, FileRemoveRequest,
    FileRemoveResponse, FileStat, FileStatRequest, FileStatResponse, FileWriteBeginRequest,
    FileWriteBeginResponse, FileWriteChunkRequest, FileWriteChunkResponse, FileWriteCommitRequest,
    FileWriteCommitResponse, FileWriteRequest, FileWriteResponse, GuestCpuMetrics, GuestMemMetrics,
    HandshakeMessage, MetricsRequest, MetricsResponse, Payload, PtyControl, PtyControlEvent,
    PtyExit, PtyInput, PtyOutput, PtyRequest, PtyResize, PtySignal, PtySize, ShutdownAction,
    ShutdownRequest, ShutdownResponse, PAYLOAD_KIND_CANCEL_RESPONSE, PAYLOAD_KIND_CANCEL_REQUEST,
    PAYLOAD_KIND_EXEC_EXIT, PAYLOAD_KIND_EXEC_REQUEST, PAYLOAD_KIND_EXEC_RESPONSE,
    PAYLOAD_KIND_EXEC_STDERR, PAYLOAD_KIND_EXEC_STDOUT, PAYLOAD_KIND_FILE_LIST_REQUEST,
    PAYLOAD_KIND_FILE_LIST_RESPONSE, PAYLOAD_KIND_FILE_READ_CHUNK, PAYLOAD_KIND_FILE_READ_REQUEST,
    PAYLOAD_KIND_FILE_READ_RESPONSE, PAYLOAD_KIND_FILE_REMOVE_REQUEST,
    PAYLOAD_KIND_FILE_REMOVE_RESPONSE, PAYLOAD_KIND_FILE_STAT_REQUEST,
    PAYLOAD_KIND_FILE_STAT_RESPONSE, PAYLOAD_KIND_FILE_WRITE_BEGIN_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_BEGIN_RESPONSE, PAYLOAD_KIND_FILE_WRITE_CHUNK_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_CHUNK_RESPONSE, PAYLOAD_KIND_FILE_WRITE_COMMIT_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_COMMIT_RESPONSE, PAYLOAD_KIND_FILE_WRITE_REQUEST,
    PAYLOAD_KIND_FILE_WRITE_RESPONSE, PAYLOAD_KIND_METRICS_REQUEST, PAYLOAD_KIND_METRICS_RESPONSE,
    PAYLOAD_KIND_PTY_CONTROL, PAYLOAD_KIND_PTY_EXIT, PAYLOAD_KIND_PTY_INPUT,
    PAYLOAD_KIND_PTY_OUTPUT, PAYLOAD_KIND_PTY_REQUEST, PAYLOAD_KIND_PTY_RESIZE,
    PAYLOAD_KIND_SHUTDOWN_REQUEST, PAYLOAD_KIND_SHUTDOWN_RESPONSE,
};

use super::*;

fn env_to_wire(env: Option<Vec<(String, String)>>) -> Vec<WireEnvVar> {
    env.unwrap_or_default()
        .into_iter()
        .map(|(key, value)| WireEnvVar { key, value })
        .collect()
}

fn env_from_wire(env: Vec<WireEnvVar>) -> Option<Vec<(String, String)>> {
    if env.is_empty() {
        None
    } else {
        Some(env.into_iter().map(|kv| (kv.key, kv.value)).collect())
    }
}

fn missing(field: &str) -> ProtoError {
    ProtoError::MalformedPayload(format!("missing protobuf field: {field}"))
}

fn timing_to_wire(t: ExecTiming) -> WireExecTiming {
    WireExecTiming {
        spawned_at_unix_ms: t.spawned_at_unix_ms,
        exited_at_unix_ms: t.exited_at_unix_ms,
        spawn_ms: t.spawn_ms,
        run_ms: t.run_ms,
    }
}

fn timing_from_wire(t: Option<WireExecTiming>) -> Result<ExecTiming, ProtoError> {
    let t = t.ok_or_else(|| missing("timing"))?;
    Ok(ExecTiming {
        spawned_at_unix_ms: t.spawned_at_unix_ms,
        exited_at_unix_ms: t.exited_at_unix_ms,
        spawn_ms: t.spawn_ms,
        run_ms: t.run_ms,
    })
}

fn exec_status_to_i32(status: ExecStatus) -> i32 {
    match status {
        ExecStatus::Completed => 0,
        ExecStatus::TimedOut => 1,
        ExecStatus::Cancelled => 2,
        ExecStatus::Failed => 3,
    }
}

fn exec_status_from_i32(value: i32) -> Result<ExecStatus, ProtoError> {
    match value {
        0 => Ok(ExecStatus::Completed),
        1 => Ok(ExecStatus::TimedOut),
        2 => Ok(ExecStatus::Cancelled),
        3 => Ok(ExecStatus::Failed),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown exec status: {value}"
        ))),
    }
}

fn shutdown_action_to_i32(action: ShutdownAction) -> i32 {
    match action {
        ShutdownAction::Exit => 0,
        ShutdownAction::Poweroff => 1,
    }
}

fn shutdown_action_from_i32(value: i32) -> Result<ShutdownAction, ProtoError> {
    match value {
        0 => Ok(ShutdownAction::Exit),
        1 => Ok(ShutdownAction::Poweroff),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown shutdown action: {value}"
        ))),
    }
}

fn cancel_status_to_i32(status: CancelStatus) -> i32 {
    match status {
        CancelStatus::Cancelled => 0,
        CancelStatus::AlreadyExited => 1,
        CancelStatus::Failed => 2,
    }
}

fn cancel_status_from_i32(value: i32) -> Result<CancelStatus, ProtoError> {
    match value {
        0 => Ok(CancelStatus::Cancelled),
        1 => Ok(CancelStatus::AlreadyExited),
        2 => Ok(CancelStatus::Failed),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown cancel status: {value}"
        ))),
    }
}

fn file_error_to_i32(error: FileError) -> i32 {
    match error {
        FileError::NotFound => 0,
        FileError::PermissionDenied => 1,
        FileError::IsADirectory => 2,
        FileError::NotADirectory => 3,
        FileError::SymlinkRejected => 4,
        FileError::TooLarge => 5,
        FileError::InvalidSequence => 6,
        FileError::Io => 7,
    }
}

fn file_error_from_i32(value: i32) -> Result<FileError, ProtoError> {
    match value {
        0 => Ok(FileError::NotFound),
        1 => Ok(FileError::PermissionDenied),
        2 => Ok(FileError::IsADirectory),
        3 => Ok(FileError::NotADirectory),
        4 => Ok(FileError::SymlinkRejected),
        5 => Ok(FileError::TooLarge),
        6 => Ok(FileError::InvalidSequence),
        7 => Ok(FileError::Io),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown file error: {value}"
        ))),
    }
}

fn opt_file_error_from_i32(value: Option<i32>) -> Result<Option<FileError>, ProtoError> {
    value.map(file_error_from_i32).transpose()
}

fn opt_file_error_to_i32(value: Option<FileError>) -> Option<i32> {
    value.map(file_error_to_i32)
}

fn file_kind_to_i32(kind: FileKind) -> i32 {
    match kind {
        FileKind::File => 0,
        FileKind::Directory => 1,
        FileKind::Symlink => 2,
        FileKind::Other => 3,
    }
}

fn file_kind_from_i32(value: i32) -> Result<FileKind, ProtoError> {
    match value {
        0 => Ok(FileKind::File),
        1 => Ok(FileKind::Directory),
        2 => Ok(FileKind::Symlink),
        3 => Ok(FileKind::Other),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown file kind: {value}"
        ))),
    }
}

fn pty_signal_to_i32(signal: PtySignal) -> i32 {
    match signal {
        PtySignal::Interrupt => 0,
        PtySignal::Terminate => 1,
        PtySignal::Hangup => 2,
        PtySignal::Kill => 3,
    }
}

fn pty_signal_from_i32(value: i32) -> Result<PtySignal, ProtoError> {
    match value {
        0 => Ok(PtySignal::Interrupt),
        1 => Ok(PtySignal::Terminate),
        2 => Ok(PtySignal::Hangup),
        3 => Ok(PtySignal::Kill),
        _ => Err(ProtoError::MalformedPayload(format!(
            "unknown pty signal: {value}"
        ))),
    }
}

fn pty_size_to_wire(size: PtySize) -> WirePtySize {
    WirePtySize {
        rows: u32::from(size.rows),
        cols: u32::from(size.cols),
        pixel_width: size.pixel_width.map(u32::from),
        pixel_height: size.pixel_height.map(u32::from),
    }
}

fn u16_field(value: u32, field: &str) -> Result<u16, ProtoError> {
    u16::try_from(value)
        .map_err(|_| ProtoError::MalformedPayload(format!("{field} does not fit in u16")))
}

fn pty_size_from_wire(size: Option<WirePtySize>) -> Result<PtySize, ProtoError> {
    let size = size.ok_or_else(|| missing("pty size"))?;
    Ok(PtySize {
        rows: u16_field(size.rows, "rows")?,
        cols: u16_field(size.cols, "cols")?,
        pixel_width: size
            .pixel_width
            .map(|v| u16_field(v, "pixel_width"))
            .transpose()?,
        pixel_height: size
            .pixel_height
            .map(|v| u16_field(v, "pixel_height"))
            .transpose()?,
    })
}

macro_rules! payload_impl {
    ($ty:ty, $kind:expr, $variant:ident, $wire:ty, $to_wire:expr, $from_wire:expr) => {
        impl Payload for $ty {
            const KIND: &'static str = $kind;

            fn into_wire(self) -> WirePayload {
                let f: fn($ty) -> $wire = $to_wire;
                WirePayload::$variant(f(self))
            }

            fn from_wire(payload: WirePayload) -> Result<Self, ProtoError> {
                match payload {
                    WirePayload::$variant(value) => {
                        let f: fn($wire) -> Result<$ty, ProtoError> = $from_wire;
                        f(value)
                    }
                    other => Err(ProtoError::MalformedPayload(format!(
                        "unexpected protobuf payload for {}: {}",
                        <$ty as Payload>::KIND,
                        payload_name(&other)
                    ))),
                }
            }
        }
    };
}

fn payload_name(payload: &WirePayload) -> &'static str {
    match payload {
        WirePayload::Handshake(_) => "handshake",
        WirePayload::ExecRequest(_) => "exec_request",
        WirePayload::ExecResponse(_) => "exec_response",
        WirePayload::ShutdownRequest(_) => "shutdown_request",
        WirePayload::ShutdownResponse(_) => "shutdown_response",
        WirePayload::CancelRequest(_) => "cancel_request",
        WirePayload::CancelAck(_) => "cancel_response",
        WirePayload::MetricsRequest(_) => "metrics_request",
        WirePayload::MetricsResponse(_) => "metrics_response",
        WirePayload::FileReadRequest(_) => "file_read_request",
        WirePayload::FileReadResponse(_) => "file_read_response",
        WirePayload::FileReadChunk(_) => "file_read_chunk",
        WirePayload::FileWriteRequest(_) => "file_write_request",
        WirePayload::FileWriteResponse(_) => "file_write_response",
        WirePayload::FileListRequest(_) => "file_list_request",
        WirePayload::FileListResponse(_) => "file_list_response",
        WirePayload::FileStatRequest(_) => "file_stat_request",
        WirePayload::FileStatResponse(_) => "file_stat_response",
        WirePayload::FileRemoveRequest(_) => "file_remove_request",
        WirePayload::FileRemoveResponse(_) => "file_remove_response",
        WirePayload::FileWriteBeginRequest(_) => "file_write_begin_request",
        WirePayload::FileWriteBeginResponse(_) => "file_write_begin_response",
        WirePayload::FileWriteChunkRequest(_) => "file_write_chunk_request",
        WirePayload::FileWriteChunkResponse(_) => "file_write_chunk_response",
        WirePayload::FileWriteCommitRequest(_) => "file_write_commit_request",
        WirePayload::FileWriteCommitResponse(_) => "file_write_commit_response",
        WirePayload::ExecStdout(_) => "exec_stdout",
        WirePayload::ExecStderr(_) => "exec_stderr",
        WirePayload::ExecExit(_) => "exec_exit",
        WirePayload::PtyRequest(_) => "pty_request",
        WirePayload::PtyInput(_) => "pty_input",
        WirePayload::PtyOutput(_) => "pty_output",
        WirePayload::PtyResize(_) => "pty_resize",
        WirePayload::PtyControl(_) => "pty_control",
        WirePayload::PtyExit(_) => "pty_exit",
    }
}

payload_impl!(
    HandshakeMessage,
    "handshake",
    Handshake,
    WireHandshakeMessage,
    |v| WireHandshakeMessage { version: v.version },
    |v| Ok(HandshakeMessage { version: v.version })
);

payload_impl!(
    ExecRequest,
    PAYLOAD_KIND_EXEC_REQUEST,
    ExecRequest,
    WireExecRequest,
    |v| WireExecRequest {
        program: v.program,
        args: v.args,
        cwd: v.cwd,
        env: env_to_wire(v.env),
        stdin: v.stdin,
        timeout_ms: v.timeout_ms,
        streaming: v.streaming,
    },
    |v| Ok(ExecRequest {
        program: v.program,
        args: v.args,
        cwd: v.cwd,
        env: env_from_wire(v.env),
        stdin: v.stdin,
        timeout_ms: v.timeout_ms,
        streaming: v.streaming,
    })
);

payload_impl!(
    ExecResponse,
    PAYLOAD_KIND_EXEC_RESPONSE,
    ExecResponse,
    WireExecResponse,
    |v| WireExecResponse {
        status: exec_status_to_i32(v.status),
        exit_code: v.exit_code,
        stdout: v.stdout,
        stderr: v.stderr,
        truncated: v.truncated,
        timing: Some(timing_to_wire(v.timing)),
    },
    |v| Ok(ExecResponse {
        status: exec_status_from_i32(v.status)?,
        exit_code: v.exit_code,
        stdout: v.stdout,
        stderr: v.stderr,
        truncated: v.truncated,
        timing: timing_from_wire(v.timing)?,
    })
);

payload_impl!(
    ShutdownRequest,
    PAYLOAD_KIND_SHUTDOWN_REQUEST,
    ShutdownRequest,
    WireShutdownRequest,
    |v| WireShutdownRequest { reason: v.reason },
    |v| Ok(ShutdownRequest { reason: v.reason })
);

payload_impl!(
    ShutdownResponse,
    PAYLOAD_KIND_SHUTDOWN_RESPONSE,
    ShutdownResponse,
    WireShutdownResponse,
    |v| WireShutdownResponse {
        action: shutdown_action_to_i32(v.action),
    },
    |v| Ok(ShutdownResponse {
        action: shutdown_action_from_i32(v.action)?,
    })
);

payload_impl!(
    CancelRequest,
    PAYLOAD_KIND_CANCEL_REQUEST,
    CancelRequest,
    WireCancelRequest,
    |v| WireCancelRequest {
        request_id: v.request_id,
    },
    |v| Ok(CancelRequest {
        request_id: v.request_id,
    })
);

payload_impl!(
    CancelResponse,
    PAYLOAD_KIND_CANCEL_RESPONSE,
    CancelAck,
    WireCancelAck,
    |v| WireCancelAck {
        request_id: v.request_id,
        status: cancel_status_to_i32(v.status),
    },
    |v| Ok(CancelResponse {
        request_id: v.request_id,
        status: cancel_status_from_i32(v.status)?,
    })
);

payload_impl!(
    MetricsRequest,
    PAYLOAD_KIND_METRICS_REQUEST,
    MetricsRequest,
    WireMetricsRequest,
    |_| WireMetricsRequest {},
    |_| Ok(MetricsRequest {})
);

payload_impl!(
    MetricsResponse,
    PAYLOAD_KIND_METRICS_RESPONSE,
    MetricsResponse,
    WireMetricsResponse,
    |v| WireMetricsResponse {
        cpu: Some(WireGuestCpuMetrics {
            user_ticks: v.cpu.user_ticks,
            nice_ticks: v.cpu.nice_ticks,
            system_ticks: v.cpu.system_ticks,
            idle_ticks: v.cpu.idle_ticks,
            iowait_ticks: v.cpu.iowait_ticks,
            irq_ticks: v.cpu.irq_ticks,
            softirq_ticks: v.cpu.softirq_ticks,
            steal_ticks: v.cpu.steal_ticks,
            guest_ticks: v.cpu.guest_ticks,
            guest_nice_ticks: v.cpu.guest_nice_ticks,
            total_ticks: v.cpu.total_ticks,
        }),
        mem: Some(WireGuestMemMetrics {
            mem_total_bytes: v.mem.mem_total_bytes,
            mem_available_bytes: v.mem.mem_available_bytes,
            mem_free_bytes: v.mem.mem_free_bytes,
            buffers_bytes: v.mem.buffers_bytes,
            cached_bytes: v.mem.cached_bytes,
            swap_total_bytes: v.mem.swap_total_bytes,
            swap_free_bytes: v.mem.swap_free_bytes,
        }),
        requests_total: v.requests_total,
        errors_total: v.errors_total,
    },
    |v| {
        let cpu = v.cpu.ok_or_else(|| missing("cpu"))?;
        let mem = v.mem.ok_or_else(|| missing("mem"))?;
        Ok(MetricsResponse {
            cpu: GuestCpuMetrics {
                user_ticks: cpu.user_ticks,
                nice_ticks: cpu.nice_ticks,
                system_ticks: cpu.system_ticks,
                idle_ticks: cpu.idle_ticks,
                iowait_ticks: cpu.iowait_ticks,
                irq_ticks: cpu.irq_ticks,
                softirq_ticks: cpu.softirq_ticks,
                steal_ticks: cpu.steal_ticks,
                guest_ticks: cpu.guest_ticks,
                guest_nice_ticks: cpu.guest_nice_ticks,
                total_ticks: cpu.total_ticks,
            },
            mem: GuestMemMetrics {
                mem_total_bytes: mem.mem_total_bytes,
                mem_available_bytes: mem.mem_available_bytes,
                mem_free_bytes: mem.mem_free_bytes,
                buffers_bytes: mem.buffers_bytes,
                cached_bytes: mem.cached_bytes,
                swap_total_bytes: mem.swap_total_bytes,
                swap_free_bytes: mem.swap_free_bytes,
            },
            requests_total: v.requests_total,
            errors_total: v.errors_total,
        })
    }
);

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

payload_impl!(
    ExecStdout,
    PAYLOAD_KIND_EXEC_STDOUT,
    ExecStdout,
    WireExecStreamChunk,
    |v| WireExecStreamChunk {
        seq: v.seq,
        bytes: v.bytes,
    },
    |v| Ok(ExecStdout {
        seq: v.seq,
        bytes: v.bytes,
    })
);

payload_impl!(
    ExecStderr,
    PAYLOAD_KIND_EXEC_STDERR,
    ExecStderr,
    WireExecStreamChunk,
    |v| WireExecStreamChunk {
        seq: v.seq,
        bytes: v.bytes,
    },
    |v| Ok(ExecStderr {
        seq: v.seq,
        bytes: v.bytes,
    })
);

payload_impl!(
    ExecExit,
    PAYLOAD_KIND_EXEC_EXIT,
    ExecExit,
    WireExecExit,
    |v| WireExecExit {
        status: exec_status_to_i32(v.status),
        exit_code: v.exit_code,
        total_stdout_bytes: v.total_stdout_bytes,
        total_stderr_bytes: v.total_stderr_bytes,
        truncated: v.truncated,
        timing: Some(timing_to_wire(v.timing)),
    },
    |v| Ok(ExecExit {
        status: exec_status_from_i32(v.status)?,
        exit_code: v.exit_code,
        total_stdout_bytes: v.total_stdout_bytes,
        total_stderr_bytes: v.total_stderr_bytes,
        truncated: v.truncated,
        timing: timing_from_wire(v.timing)?,
    })
);

payload_impl!(
    PtyRequest,
    PAYLOAD_KIND_PTY_REQUEST,
    PtyRequest,
    WirePtyRequest,
    |v| WirePtyRequest {
        program: v.program,
        args: v.args,
        cwd: v.cwd,
        env: env_to_wire(v.env),
        timeout_ms: v.timeout_ms,
        size: Some(pty_size_to_wire(v.size)),
    },
    |v| Ok(PtyRequest {
        program: v.program,
        args: v.args,
        cwd: v.cwd,
        env: env_from_wire(v.env),
        timeout_ms: v.timeout_ms,
        size: pty_size_from_wire(v.size)?,
    })
);

payload_impl!(
    PtyInput,
    PAYLOAD_KIND_PTY_INPUT,
    PtyInput,
    WirePtyBytes,
    |v| WirePtyBytes {
        seq: v.seq,
        bytes: v.bytes,
    },
    |v| Ok(PtyInput {
        seq: v.seq,
        bytes: v.bytes,
    })
);

payload_impl!(
    PtyOutput,
    PAYLOAD_KIND_PTY_OUTPUT,
    PtyOutput,
    WirePtyBytes,
    |v| WirePtyBytes {
        seq: v.seq,
        bytes: v.bytes,
    },
    |v| Ok(PtyOutput {
        seq: v.seq,
        bytes: v.bytes,
    })
);

payload_impl!(
    PtyResize,
    PAYLOAD_KIND_PTY_RESIZE,
    PtyResize,
    WirePtyResize,
    |v| WirePtyResize {
        seq: v.seq,
        size: Some(pty_size_to_wire(v.size)),
    },
    |v| Ok(PtyResize {
        seq: v.seq,
        size: pty_size_from_wire(v.size)?,
    })
);

payload_impl!(
    PtyControl,
    PAYLOAD_KIND_PTY_CONTROL,
    PtyControl,
    WirePtyControl,
    |v| WirePtyControl {
        seq: v.seq,
        event: Some(match v.event {
            PtyControlEvent::Eof => WirePtyControlEvent::Eof(true),
            PtyControlEvent::Signal { signal } =>
                WirePtyControlEvent::Signal(pty_signal_to_i32(signal)),
        }),
    },
    |v| {
        let event = match v.event.ok_or_else(|| missing("pty control event"))? {
            WirePtyControlEvent::Eof(_) => PtyControlEvent::Eof,
            WirePtyControlEvent::Signal(signal) => PtyControlEvent::Signal {
                signal: pty_signal_from_i32(signal)?,
            },
        };
        Ok(PtyControl { seq: v.seq, event })
    }
);

payload_impl!(
    PtyExit,
    PAYLOAD_KIND_PTY_EXIT,
    PtyExit,
    WirePtyExit,
    |v| WirePtyExit {
        status: exec_status_to_i32(v.status),
        exit_code: v.exit_code,
        exit_signal: v.exit_signal,
        total_input_bytes: v.total_input_bytes,
        total_output_bytes: v.total_output_bytes,
        truncated: v.truncated,
        timing: Some(timing_to_wire(v.timing)),
    },
    |v| Ok(PtyExit {
        status: exec_status_from_i32(v.status)?,
        exit_code: v.exit_code,
        exit_signal: v.exit_signal,
        total_input_bytes: v.total_input_bytes,
        total_output_bytes: v.total_output_bytes,
        truncated: v.truncated,
        timing: timing_from_wire(v.timing)?,
    })
);
