//! File-operation methods for [`RunningSandbox`].

use std::io::Read;
use std::sync::atomic::Ordering;

use m80_proto::{
    DirEntry, Envelope, FileListRequest, FileListResponse, FileMkdirRequest, FileMkdirResponse,
    FileReadChunk, FileReadRequest, FileRemoveRequest, FileRemoveResponse, FileStat,
    FileStatRequest, FileStatResponse, FileWriteBeginRequest, FileWriteBeginResponse,
    FileWriteChunkRequest, FileWriteChunkResponse, FileWriteCommitRequest, FileWriteCommitResponse,
    FileWriteRequest, FileWriteResponse, PAYLOAD_KIND_FILE_READ_CHUNK,
};

use crate::error::{ConfigError, FcError, WireProtocolError};
use crate::layout::VSOCK_SOCKET;
use crate::lifecycle::exec::{request_id_for, send_envelope_with_open_retry};
use crate::lifecycle::monotonic_ns;
use crate::types::RunningSandbox;

impl RunningSandbox {
    /// Read a guest file directly through m80-guestd.
    pub fn read_file(
        &mut self,
        path: impl Into<String>,
        max_bytes: Option<u64>,
    ) -> Result<(Vec<u8>, bool), FcError> {
        self.prepare_fileop_activity()?;
        let vsock_uds = self.jail.jail_root().join(VSOCK_SOCKET);
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), "file_read");
        let envelope = Envelope::with_request_id(
            FileReadRequest {
                path: path.into(),
                max_bytes,
            },
            request_id.clone(),
        );
        let firecracker_pid = self.firecracker.firecracker_pid();
        let mut channel = send_envelope_with_open_retry(
            &vsock_uds,
            &self.vm_id,
            firecracker_pid,
            "file read",
            &envelope,
        )?;
        let mut bytes = Vec::new();
        let mut expected_seq = 0u64;

        loop {
            let frame = match channel.recv_raw() {
                Ok(frame) => frame,
                Err(e) => {
                    let err = super::protocol::recv_error(e, "file read", firecracker_pid);
                    crate::diagnostics::record_protocol_error(
                        &mut self.diagnostics,
                        &self.vm_id,
                        &request_id,
                        "file_read_chunk",
                        &err,
                    );
                    return Err(err);
                }
            };
            if frame.kind != PAYLOAD_KIND_FILE_READ_CHUNK {
                let err = super::protocol::unexpected_frame(
                    "file read",
                    PAYLOAD_KIND_FILE_READ_CHUNK,
                    frame.kind,
                );
                crate::diagnostics::record_protocol_error(
                    &mut self.diagnostics,
                    &self.vm_id,
                    &request_id,
                    "file_read_chunk",
                    &err,
                );
                return Err(err);
            }
            let chunk = frame
                .decode::<FileReadChunk>()
                .map_err(super::protocol::proto_error)?
                .payload;
            if chunk.seq != expected_seq {
                let err = super::protocol::sequence_mismatch("file_read", expected_seq, chunk.seq);
                crate::diagnostics::record_protocol_error(
                    &mut self.diagnostics,
                    &self.vm_id,
                    &request_id,
                    "file_read_chunk",
                    &err,
                );
                return Err(err);
            }
            self.last_activity_ns
                .store(monotonic_ns(), Ordering::Relaxed);
            if let Some(error) = chunk.error {
                return Err(FcError::FileOp(error));
            }
            bytes.extend_from_slice(&chunk.bytes);
            if chunk.done {
                return Ok((bytes, chunk.truncated));
            }
            expected_seq = expected_seq.checked_add(1).ok_or(FcError::Protocol(
                WireProtocolError::SequenceOverflow {
                    stream: "file_read",
                },
            ))?;
        }
    }

    /// Write a guest file directly through m80-guestd. Parent directory must exist.
    pub fn write_file(
        &mut self,
        path: impl Into<String>,
        bytes: Vec<u8>,
        mode: Option<u32>,
    ) -> Result<u64, FcError> {
        let response: FileWriteResponse = self.fileop_round_trip(
            FileWriteRequest {
                path: path.into(),
                bytes,
                mode,
            },
            "file_write",
        )?;
        fileop_result(response.error)?;
        Ok(response.bytes_written)
    }

    /// List one guest directory level.
    pub fn list_dir(&mut self, path: impl Into<String>) -> Result<Vec<DirEntry>, FcError> {
        let response: FileListResponse =
            self.fileop_round_trip(FileListRequest { path: path.into() }, "file_list")?;
        fileop_result(response.error)?;
        Ok(response.entries)
    }

    /// Stat a guest path without following the final symlink component.
    pub fn stat_file(&mut self, path: impl Into<String>) -> Result<FileStat, FcError> {
        let response: FileStatResponse =
            self.fileop_round_trip(FileStatRequest { path: path.into() }, "file_stat")?;
        fileop_result(response.error)?;
        response
            .stat
            .ok_or(FcError::Protocol(WireProtocolError::MissingField {
                context: "file_stat response",
                field: "stat",
            }))
    }

    /// Remove one non-directory guest path.
    pub fn remove_file(&mut self, path: impl Into<String>) -> Result<(), FcError> {
        let response: FileRemoveResponse =
            self.fileop_round_trip(FileRemoveRequest { path: path.into() }, "file_remove")?;
        fileop_result(response.error)?;
        if response.removed {
            Ok(())
        } else {
            Err(FcError::Protocol(WireProtocolError::PeerRejected {
                context: "file_remove response",
                detail: "reported no removal and no error".to_owned(),
            }))
        }
    }

    /// Create a guest directory directly through m80-guestd.
    pub fn create_dir(
        &mut self,
        path: impl Into<String>,
        mode: Option<u32>,
        recursive: bool,
    ) -> Result<bool, FcError> {
        let response: FileMkdirResponse = self.fileop_round_trip(
            FileMkdirRequest {
                path: path.into(),
                mode,
                recursive,
            },
            "file_mkdir",
        )?;
        fileop_result(response.error)?;
        Ok(response.created)
    }

    /// Upload a guest file through the chunked file-write protocol.
    pub fn upload_file_chunked(
        &mut self,
        path: impl Into<String>,
        mode: Option<u32>,
        mut reader: impl Read,
        chunk_size: usize,
    ) -> Result<u64, FcError> {
        if chunk_size == 0 {
            return Err(FcError::Config(ConfigError::InvalidValue {
                field: "chunk_size",
                reason: "must be > 0".into(),
            }));
        }
        self.prepare_fileop_activity()?;
        let vsock_uds = self.jail.jail_root().join(VSOCK_SOCKET);
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), "file_upload");
        let begin = Envelope::with_request_id(
            FileWriteBeginRequest {
                path: path.into(),
                mode,
            },
            request_id.clone(),
        );
        let firecracker_pid = self.firecracker.firecracker_pid();
        let mut channel = send_envelope_with_open_retry(
            &vsock_uds,
            &self.vm_id,
            firecracker_pid,
            "file upload begin",
            &begin,
        )?;
        let begin_response: Envelope<FileWriteBeginResponse> =
            recv_fileop(&mut channel, "file upload begin", firecracker_pid)?;
        let begin_response = begin_response.payload;
        fileop_result(begin_response.error)?;
        let upload_id =
            begin_response
                .upload_id
                .ok_or(FcError::Protocol(WireProtocolError::MissingField {
                    context: "file upload begin response",
                    field: "upload_id",
                }))?;

        let mut seq = 0u64;
        let mut buf = vec![0u8; chunk_size];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|source| FcError::FileUploadReadFailed { source })?;
            if n == 0 {
                break;
            }
            let chunk = FileWriteChunkRequest {
                upload_id: upload_id.clone(),
                seq,
                bytes: buf[..n].to_vec(),
            };
            channel.send(&Envelope::with_request_id(chunk, request_id.clone()))?;
            let response: Envelope<FileWriteChunkResponse> =
                recv_fileop(&mut channel, "file upload chunk", firecracker_pid)?;
            let response = response.payload;
            fileop_result(response.error)?;
            validate_chunk_ack(&upload_id, seq, &response)?;
            seq = seq.checked_add(1).ok_or(FcError::Protocol(
                WireProtocolError::SequenceOverflow {
                    stream: "file_upload",
                },
            ))?;
        }

        channel.send(&Envelope::with_request_id(
            FileWriteCommitRequest { upload_id },
            request_id,
        ))?;
        let response: Envelope<FileWriteCommitResponse> =
            recv_fileop(&mut channel, "file upload commit", firecracker_pid)?;
        let response = response.payload;
        fileop_result(response.error)?;
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        Ok(response.bytes_written)
    }

    fn fileop_round_trip<T, U>(&mut self, payload: T, kind: &str) -> Result<U, FcError>
    where
        T: m80_proto::Payload + Clone,
        U: m80_proto::Payload + Clone,
    {
        self.prepare_fileop_activity()?;
        let vsock_uds = self.jail.jail_root().join(VSOCK_SOCKET);
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), kind);
        let envelope = Envelope::with_request_id(payload, request_id);
        let firecracker_pid = self.firecracker.firecracker_pid();
        let mut channel = send_envelope_with_open_retry(
            &vsock_uds,
            &self.vm_id,
            firecracker_pid,
            "file operation",
            &envelope,
        )?;
        let frame: Envelope<U> = recv_fileop(&mut channel, "file operation", firecracker_pid)?;
        let response = frame.payload;
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        Ok(response)
    }

    fn prepare_fileop_activity(&self) -> Result<(), FcError> {
        if self.idle_timed_out.load(Ordering::Relaxed) {
            return Err(FcError::IdleTimedOut);
        }
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        Ok(())
    }
}

fn fileop_result(error: Option<m80_proto::FileError>) -> Result<(), FcError> {
    match error {
        None => Ok(()),
        Some(error) => Err(FcError::FileOp(error)),
    }
}

fn validate_chunk_ack(
    upload_id: &str,
    expected_seq: u64,
    response: &FileWriteChunkResponse,
) -> Result<(), FcError> {
    if response.upload_id != upload_id {
        return Err(super::protocol::unexpected_frame(
            "file upload chunk",
            "matching upload_id",
            response.upload_id.clone(),
        ));
    }
    if response.seq != expected_seq {
        return Err(super::protocol::sequence_mismatch(
            "file_upload",
            expected_seq,
            response.seq,
        ));
    }
    Ok(())
}

fn recv_fileop<T>(
    channel: &mut m80_vsock::Channel,
    context: &'static str,
    firecracker_pid: u32,
) -> Result<Envelope<T>, FcError>
where
    T: m80_proto::Payload,
{
    channel
        .recv()
        .map_err(|e| super::protocol::recv_error(e, context, firecracker_pid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_ack_accepts_matching_upload_id_and_sequence() {
        let ack = FileWriteChunkResponse {
            upload_id: "u1".into(),
            seq: 7,
            bytes_written: 3,
            error: None,
        };

        validate_chunk_ack("u1", 7, &ack).unwrap();
    }

    #[test]
    fn chunk_ack_rejects_wrong_upload_id() {
        let ack = FileWriteChunkResponse {
            upload_id: "u2".into(),
            seq: 0,
            bytes_written: 3,
            error: None,
        };

        let err = validate_chunk_ack("u1", 0, &ack).unwrap_err();

        assert!(matches!(
            err,
            FcError::Protocol(crate::error::WireProtocolError::UnexpectedFrame {
                context: "file upload chunk",
                expected: "matching upload_id",
                got
            }) if got == "u2"
        ));
    }

    #[test]
    fn chunk_ack_rejects_wrong_sequence() {
        let ack = FileWriteChunkResponse {
            upload_id: "u1".into(),
            seq: 9,
            bytes_written: 3,
            error: None,
        };

        let err = validate_chunk_ack("u1", 8, &ack).unwrap_err();

        assert!(matches!(
            err,
            FcError::Protocol(crate::error::WireProtocolError::SequenceMismatch {
                stream: "file_upload",
                expected: 8,
                got: 9
            })
        ));
    }
}
