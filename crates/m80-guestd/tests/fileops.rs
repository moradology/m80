use std::io::{BufReader, Cursor};
use std::os::unix::fs::PermissionsExt;

use m80_proto::{
    read_frame, write_frame, DirEntry, Envelope, FileError, FileKind, FileListResponse,
    FileReadChunk, FileReadRequest, FileRemoveRequest, FileRemoveResponse, FileStatRequest,
    FileStatResponse, FileWriteBeginRequest, FileWriteBeginResponse, FileWriteChunkRequest,
    FileWriteChunkResponse, FileWriteCommitRequest, FileWriteCommitResponse, FileWriteRequest,
    FileWriteResponse,
};

fn frame<T: m80_proto::Payload + Clone>(payload: T) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &Envelope::new(payload)).unwrap();
    bytes
}

fn handle(input: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    m80_guestd::connection::handle_connection_with_reader_ready(
        BufReader::new(Cursor::new(input)),
        &mut out,
        |_| true,
    )
    .unwrap();
    out
}

fn read_one<T: m80_proto::Payload + Clone>(bytes: Vec<u8>) -> T {
    let env: Envelope<T> = read_frame(&mut Cursor::new(bytes)).unwrap();
    env.payload
}

fn read_file_chunks(bytes: Vec<u8>) -> Vec<FileReadChunk> {
    let mut cursor = Cursor::new(bytes);
    let mut chunks = Vec::new();
    loop {
        let env: Envelope<FileReadChunk> = read_frame(&mut cursor).unwrap();
        let done = env.payload.done;
        chunks.push(env.payload);
        if done {
            return chunks;
        }
    }
}

fn collect_read(chunks: &[FileReadChunk]) -> (Vec<u8>, bool, Option<FileError>) {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut error = None;
    for chunk in chunks {
        bytes.extend_from_slice(&chunk.bytes);
        truncated = chunk.truncated;
        error = chunk.error;
    }
    (bytes, truncated, error)
}

#[test]
fn file_read_returns_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, b"hello").unwrap();

    let chunks = read_file_chunks(handle(frame(FileReadRequest {
        path: path.display().to_string(),
        max_bytes: None,
    })));
    let (bytes, truncated, error) = collect_read(&chunks);

    assert_eq!(bytes, b"hello");
    assert!(!truncated);
    assert_eq!(error, None);
    assert_eq!(chunks.last().unwrap().seq, 1);
}

#[test]
fn file_read_honors_max_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, b"abcdef").unwrap();

    let chunks = read_file_chunks(handle(frame(FileReadRequest {
        path: path.display().to_string(),
        max_bytes: Some(3),
    })));
    let (bytes, truncated, error) = collect_read(&chunks);

    assert_eq!(bytes, b"abc");
    assert!(truncated);
    assert_eq!(error, None);
}

#[test]
fn file_read_streams_large_file_in_multiple_chunks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.bin");
    let mut expected = Vec::with_capacity(5 * 1024 * 1024 + 123);
    for i in 0..(5 * 1024 * 1024 + 123) {
        expected.push((i % 251) as u8);
    }
    std::fs::write(&path, &expected).unwrap();

    let chunks = read_file_chunks(handle(frame(FileReadRequest {
        path: path.display().to_string(),
        max_bytes: Some(expected.len() as u64),
    })));
    let (bytes, truncated, error) = collect_read(&chunks);

    assert_eq!(bytes, expected);
    assert!(!truncated);
    assert_eq!(error, None);
    assert!(chunks.len() > 2);
    assert!(chunks.last().unwrap().done);
}

#[test]
fn file_read_rejects_final_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    std::fs::write(&target, b"secret").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let chunks = read_file_chunks(handle(frame(FileReadRequest {
        path: link.display().to_string(),
        max_bytes: None,
    })));
    let (bytes, truncated, error) = collect_read(&chunks);

    assert_eq!(error, Some(FileError::SymlinkRejected));
    assert!(!truncated);
    assert!(bytes.is_empty());
}

#[test]
fn file_write_creates_file_with_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.txt");

    let response: FileWriteResponse = read_one(handle(frame(FileWriteRequest {
        path: path.display().to_string(),
        bytes: b"hello".to_vec(),
        mode: Some(0o640),
    })));

    assert_eq!(response.error, None);
    assert_eq!(response.bytes_written, 5);
    assert_eq!(std::fs::read(&path).unwrap(), b"hello");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn file_list_reports_symlink_kind_without_following() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    std::fs::write(&target, b"abc").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let response: FileListResponse = read_one(handle(frame(m80_proto::FileListRequest {
        path: dir.path().display().to_string(),
    })));

    assert_eq!(response.error, None);
    assert!(response.entries.contains(&DirEntry {
        name: "link.txt".into(),
        kind: FileKind::Symlink,
        size: target.display().to_string().len() as u64,
    }));
}

#[test]
fn file_stat_returns_mode_and_kind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, b"abc").unwrap();

    let response: FileStatResponse = read_one(handle(frame(FileStatRequest {
        path: path.display().to_string(),
    })));

    let stat = response.stat.unwrap();
    assert_eq!(response.error, None);
    assert_eq!(stat.kind, FileKind::File);
    assert_eq!(stat.size, 3);
    assert_ne!(stat.mode & 0o170000, 0);
}

#[test]
fn file_remove_deletes_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gone.txt");
    std::fs::write(&path, b"abc").unwrap();

    let response: FileRemoveResponse = read_one(handle(frame(FileRemoveRequest {
        path: path.display().to_string(),
    })));

    assert!(response.removed);
    assert_eq!(response.error, None);
    assert!(!path.exists());
}

#[test]
fn file_remove_rejects_directory() {
    let dir = tempfile::tempdir().unwrap();

    let response: FileRemoveResponse = read_one(handle(frame(FileRemoveRequest {
        path: dir.path().display().to_string(),
    })));

    assert!(!response.removed);
    assert_eq!(response.error, Some(FileError::IsADirectory));
}

#[test]
fn chunked_upload_writes_chunks_and_commits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.bin");
    let mut input = Vec::new();
    input.extend(frame(FileWriteBeginRequest {
        path: path.display().to_string(),
        mode: Some(0o600),
    }));
    input.extend(frame(FileWriteChunkRequest {
        upload_id: "u1".into(),
        seq: 0,
        bytes: b"abc".to_vec(),
    }));
    input.extend(frame(FileWriteChunkRequest {
        upload_id: "u1".into(),
        seq: 1,
        bytes: b"def".to_vec(),
    }));
    input.extend(frame(FileWriteCommitRequest {
        upload_id: "u1".into(),
    }));

    let output = handle(input);
    let mut cursor = Cursor::new(output);
    let begin: Envelope<FileWriteBeginResponse> = read_frame(&mut cursor).unwrap();
    let chunk0: Envelope<FileWriteChunkResponse> = read_frame(&mut cursor).unwrap();
    let chunk1: Envelope<FileWriteChunkResponse> = read_frame(&mut cursor).unwrap();
    let commit: Envelope<FileWriteCommitResponse> = read_frame(&mut cursor).unwrap();

    assert_eq!(begin.payload.upload_id.as_deref(), Some("u1"));
    assert_eq!(chunk0.payload.bytes_written, 3);
    assert_eq!(chunk1.payload.seq, 1);
    assert_eq!(commit.payload.bytes_written, 6);
    assert_eq!(std::fs::read(&path).unwrap(), b"abcdef");
}
