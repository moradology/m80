use m80_proto::{
    read_frame, write_frame, DirEntry, Envelope, FileError, FileKind, FileListRequest,
    FileListResponse, FileMkdirRequest, FileMkdirResponse, FileReadChunk, FileReadRequest,
    FileReadResponse, FileRemoveRequest, FileRemoveResponse, FileStat, FileStatRequest,
    FileStatResponse, FileWriteBeginRequest, FileWriteBeginResponse, FileWriteChunkRequest,
    FileWriteChunkResponse, FileWriteCommitRequest, FileWriteCommitResponse, FileWriteRequest,
    FileWriteResponse, PAYLOAD_KIND_FILE_MKDIR_REQUEST, PAYLOAD_KIND_FILE_READ_REQUEST,
};

fn round_trip<T>(payload: T) -> Envelope<T>
where
    T: m80_proto::Payload + Clone,
{
    let env = Envelope::with_request_id(payload, "req-file".to_owned());
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &env).unwrap();
    read_frame(&mut std::io::Cursor::new(bytes)).unwrap()
}

#[test]
fn file_read_request_round_trips_with_kind() {
    let back = round_trip(FileReadRequest {
        path: "/workspace/a.txt".into(),
        max_bytes: Some(7),
    });

    assert_eq!(back.kind, PAYLOAD_KIND_FILE_READ_REQUEST);
    assert_eq!(back.payload.path, "/workspace/a.txt");
    assert_eq!(back.payload.max_bytes, Some(7));
}

#[test]
fn file_read_response_round_trips_bytes_error_and_truncation() {
    let back = round_trip(FileReadResponse {
        bytes: b"abcdefg".to_vec(),
        truncated: true,
        error: Some(FileError::TooLarge),
    });

    assert_eq!(back.payload.bytes, b"abcdefg");
    assert!(back.payload.truncated);
    assert_eq!(back.payload.error, Some(FileError::TooLarge));
}

#[test]
fn file_write_request_round_trips_mode_and_bytes() {
    let back = round_trip(FileWriteRequest {
        path: "/workspace/out.bin".into(),
        bytes: vec![0, 1, 2, 3],
        mode: Some(0o640),
    });

    assert_eq!(back.payload.bytes, vec![0, 1, 2, 3]);
    assert_eq!(back.payload.mode, Some(0o640));
}

#[test]
fn file_write_response_round_trips_error_none() {
    let back = round_trip(FileWriteResponse {
        bytes_written: 4,
        error: None,
    });

    assert_eq!(back.payload.bytes_written, 4);
    assert_eq!(back.payload.error, None);
}

#[test]
fn file_list_response_round_trips_entry_kinds() {
    let back = round_trip(FileListResponse {
        entries: vec![DirEntry {
            name: "link".into(),
            kind: FileKind::Symlink,
            size: 5,
        }],
        error: None,
    });

    assert_eq!(back.payload.entries[0].kind, FileKind::Symlink);
}

#[test]
fn file_stat_response_round_trips_mode_and_mtime() {
    let back = round_trip(FileStatResponse {
        stat: Some(FileStat {
            kind: FileKind::File,
            size: 11,
            mtime_unix_ms: 1234,
            mode: 0o100644,
        }),
        error: None,
    });

    let stat = back.payload.stat.unwrap();
    assert_eq!(stat.size, 11);
    assert_eq!(stat.mode, 0o100644);
}

#[test]
fn file_remove_response_round_trips_not_found() {
    let back = round_trip(FileRemoveResponse {
        removed: false,
        error: Some(FileError::NotFound),
    });

    assert_eq!(back.payload.error, Some(FileError::NotFound));
}

#[test]
fn file_mkdir_request_response_round_trip() {
    let req = round_trip(FileMkdirRequest {
        path: "/workspace/output".into(),
        mode: Some(0o755),
        recursive: true,
    });
    assert_eq!(req.kind, PAYLOAD_KIND_FILE_MKDIR_REQUEST);
    assert_eq!(req.payload.path, "/workspace/output");
    assert_eq!(req.payload.mode, Some(0o755));
    assert!(req.payload.recursive);

    let resp = round_trip(FileMkdirResponse {
        created: true,
        error: None,
    });
    assert!(resp.payload.created);
    assert_eq!(resp.payload.error, None);
}

#[test]
fn chunked_upload_payloads_round_trip() {
    let begin = round_trip(FileWriteBeginRequest {
        path: "/workspace/big.bin".into(),
        mode: Some(0o600),
    });
    assert_eq!(begin.payload.mode, Some(0o600));

    let begin_resp = round_trip(FileWriteBeginResponse {
        upload_id: Some("u1".into()),
        error: None,
    });
    assert_eq!(begin_resp.payload.upload_id.as_deref(), Some("u1"));

    let chunk = round_trip(FileWriteChunkRequest {
        upload_id: "u1".into(),
        seq: 2,
        bytes: b"chunk".to_vec(),
    });
    assert_eq!(chunk.payload.bytes, b"chunk");

    let chunk_resp = round_trip(FileWriteChunkResponse {
        upload_id: "u1".into(),
        seq: 2,
        bytes_written: 0,
        error: Some(FileError::InvalidSequence),
    });
    assert_eq!(chunk_resp.payload.bytes_written, 0);
    assert_eq!(chunk_resp.payload.error, Some(FileError::InvalidSequence));

    let commit = round_trip(FileWriteCommitRequest {
        upload_id: "u1".into(),
    });
    assert_eq!(commit.payload.upload_id, "u1");

    let commit_resp = round_trip(FileWriteCommitResponse {
        bytes_written: 5,
        error: None,
    });
    assert_eq!(commit_resp.payload.bytes_written, 5);
}

#[test]
fn byte_heavy_file_chunks_do_not_emit_base64_payloads() {
    let payload = FileWriteChunkRequest {
        upload_id: "u1".into(),
        seq: 0,
        bytes: b"chunk".to_vec(),
    };
    let env = Envelope::with_request_id(payload, "req-file".to_owned());
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &env).unwrap();

    assert!(!bytes.windows(b"Y2h1bms=".len()).any(|w| w == b"Y2h1bms="));

    let read_back = round_trip(FileReadChunk {
        seq: 3,
        bytes: b"read-chunk".to_vec(),
        done: false,
        truncated: false,
        error: None,
    });
    assert_eq!(read_back.payload.bytes, b"read-chunk");
}

#[test]
fn request_payloads_round_trip() {
    let list = round_trip(FileListRequest {
        path: "/workspace".into(),
    });
    assert_eq!(list.payload.path, "/workspace");

    let stat = round_trip(FileStatRequest {
        path: "/workspace/a".into(),
    });
    assert_eq!(stat.payload.path, "/workspace/a");

    let remove = round_trip(FileRemoveRequest {
        path: "/workspace/a".into(),
    });
    assert_eq!(remove.payload.path, "/workspace/a");
}
