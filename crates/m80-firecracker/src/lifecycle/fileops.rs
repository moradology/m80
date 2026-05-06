//! File-operation methods for [`RunningSandbox`].

use std::io::Read;
use std::sync::atomic::Ordering;

use m80_proto::{
    DirEntry, Envelope, FileListRequest, FileListResponse, FileReadRequest, FileReadResponse,
    FileRemoveRequest, FileRemoveResponse, FileStat, FileStatRequest, FileStatResponse,
    FileWriteBeginRequest, FileWriteBeginResponse, FileWriteChunkRequest, FileWriteChunkResponse,
    FileWriteCommitRequest, FileWriteCommitResponse, FileWriteRequest, FileWriteResponse,
};

use crate::error::FcError;
use crate::lifecycle::exec::{decode_payload, request_id_for, send_envelope_with_open_retry};
use crate::lifecycle::monotonic_ns;
use crate::types::RunningSandbox;

impl RunningSandbox {
    /// Read a guest file directly through m80-guestd.
    pub fn read_file(
        &mut self,
        path: impl Into<String>,
        max_bytes: Option<u64>,
    ) -> Result<(Vec<u8>, bool), FcError> {
        let response: FileReadResponse = self.fileop_round_trip(
            FileReadRequest {
                path: path.into(),
                max_bytes,
            },
            "file_read",
        )?;
        fileop_result(response.error)?;
        Ok((response.bytes, response.truncated))
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
            .ok_or_else(|| FcError::Config("file_stat response missing stat".into()))
    }

    /// Remove one non-directory guest path.
    pub fn remove_file(&mut self, path: impl Into<String>) -> Result<(), FcError> {
        let response: FileRemoveResponse =
            self.fileop_round_trip(FileRemoveRequest { path: path.into() }, "file_remove")?;
        fileop_result(response.error)?;
        if response.removed {
            Ok(())
        } else {
            Err(FcError::Config(
                "file_remove response reported no removal and no error".into(),
            ))
        }
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
            return Err(FcError::Config("chunk_size must be > 0".into()));
        }
        self.prepare_fileop_activity()?;
        let vsock_uds = self.jail.jail_path.join("vsock.sock");
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), "file_upload");
        let begin = Envelope::with_request_id(
            FileWriteBeginRequest {
                path: path.into(),
                mode,
            },
            request_id.clone(),
        );
        let mut channel = send_envelope_with_open_retry(&vsock_uds, &self.vm_id, &begin)?;
        let begin_response: Envelope<serde_json::Value> = channel.recv()?;
        let begin_response: FileWriteBeginResponse = decode_payload(begin_response.payload)?;
        fileop_result(begin_response.error)?;
        let upload_id = begin_response.upload_id.ok_or_else(|| {
            FcError::Config("file upload begin response missing upload_id".into())
        })?;

        let mut seq = 0u64;
        let mut buf = vec![0u8; chunk_size];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let chunk = FileWriteChunkRequest {
                upload_id: upload_id.clone(),
                seq,
                bytes: buf[..n].to_vec(),
            };
            channel.send(&Envelope::with_request_id(chunk, request_id.clone()))?;
            let response: Envelope<serde_json::Value> = channel.recv()?;
            let response: FileWriteChunkResponse = decode_payload(response.payload)?;
            fileop_result(response.error)?;
            seq = seq.wrapping_add(1);
        }

        channel.send(&Envelope::with_request_id(
            FileWriteCommitRequest { upload_id },
            request_id,
        ))?;
        let response: Envelope<serde_json::Value> = channel.recv()?;
        let response: FileWriteCommitResponse = decode_payload(response.payload)?;
        fileop_result(response.error)?;
        self.last_activity_ns
            .store(monotonic_ns(), Ordering::Relaxed);
        Ok(response.bytes_written)
    }

    fn fileop_round_trip<T, U>(&mut self, payload: T, kind: &str) -> Result<U, FcError>
    where
        T: m80_proto::Payload + serde::Serialize,
        U: serde::de::DeserializeOwned,
    {
        self.prepare_fileop_activity()?;
        let vsock_uds = self.jail.jail_path.join("vsock.sock");
        let request_id = request_id_for(&self.vm_id, self.request_id.as_deref(), kind);
        let envelope = Envelope::with_request_id(payload, request_id);
        let mut channel = send_envelope_with_open_retry(&vsock_uds, &self.vm_id, &envelope)?;
        let frame: Envelope<serde_json::Value> = channel.recv()?;
        let response = decode_payload(frame.payload)?;
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
