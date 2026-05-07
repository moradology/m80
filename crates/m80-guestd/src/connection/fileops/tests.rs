use m80_proto::{FileError, FileWriteBeginRequest, FileWriteChunkRequest, FileWriteCommitRequest};

use super::{begin_upload, commit_upload, write_chunk, FileWriteCommitResponse, Uploads};

#[test]
fn dropping_upload_table_unlinks_open_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("payload.bin");
    let temp_path = dir.path().join("payload.bin.m80-upload.u1");

    let mut uploads = Uploads::default();
    let response = begin_upload(
        FileWriteBeginRequest {
            path: final_path.display().to_string(),
            mode: Some(0o600),
        },
        &mut uploads,
    );

    assert_eq!(response.upload_id.as_deref(), Some("u1"));
    assert!(temp_path.exists());
    drop(uploads);
    assert!(!temp_path.exists());
    assert!(!final_path.exists());
}

#[test]
fn begin_upload_rejects_exhausted_upload_id_space_without_creating_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("payload.bin");
    let temp_path = dir
        .path()
        .join("payload.bin.m80-upload.u18446744073709551615");

    let mut uploads = Uploads::default();
    uploads.next_id = u64::MAX;
    let response = begin_upload(
        FileWriteBeginRequest {
            path: final_path.display().to_string(),
            mode: Some(0o600),
        },
        &mut uploads,
    );

    assert_eq!(response.upload_id, None);
    assert_eq!(response.error, Some(FileError::TooLarge));
    assert_eq!(uploads.next_id, u64::MAX);
    assert!(uploads.open.is_empty());
    assert!(!temp_path.exists());
    assert!(!final_path.exists());
}

#[test]
fn unknown_upload_id_returns_not_found_without_creating_files() {
    let dir = tempfile::tempdir().unwrap();

    let chunk = write_chunk(
        FileWriteChunkRequest {
            upload_id: "missing".into(),
            seq: 7,
            bytes: b"ignored".to_vec(),
        },
        &mut Uploads::default(),
    );
    assert_eq!(chunk.upload_id, "missing");
    assert_eq!(chunk.seq, 7);
    assert_eq!(chunk.bytes_written, 0);
    assert_eq!(chunk.error, Some(FileError::NotFound));

    let commit = commit_upload(
        FileWriteCommitRequest {
            upload_id: "missing".into(),
        },
        &mut Uploads::default(),
    );
    assert_eq!(commit, not_found_commit());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn chunked_upload_rejects_sequence_gap_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("payload.bin");
    let temp_path = dir.path().join("payload.bin.m80-upload.u1");
    let mut uploads = Uploads::default();

    let response = begin_upload(
        FileWriteBeginRequest {
            path: final_path.display().to_string(),
            mode: Some(0o600),
        },
        &mut uploads,
    );
    assert_eq!(response.upload_id.as_deref(), Some("u1"));

    let rejected = write_chunk(
        FileWriteChunkRequest {
            upload_id: "u1".into(),
            seq: 1,
            bytes: b"gap".to_vec(),
        },
        &mut uploads,
    );

    assert_eq!(rejected.upload_id, "u1");
    assert_eq!(rejected.seq, 1);
    assert_eq!(rejected.bytes_written, 0);
    assert_eq!(rejected.error, Some(FileError::InvalidSequence));
    assert!(!uploads.open.contains_key("u1"));
    assert!(!temp_path.exists());
}

#[test]
fn chunked_upload_rejects_duplicate_sequence_without_mutating_total() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("payload.bin");
    let temp_path = dir.path().join("payload.bin.m80-upload.u1");
    let mut uploads = Uploads::default();

    let response = begin_upload(
        FileWriteBeginRequest {
            path: final_path.display().to_string(),
            mode: Some(0o600),
        },
        &mut uploads,
    );
    assert_eq!(response.upload_id.as_deref(), Some("u1"));

    let first = write_chunk(
        FileWriteChunkRequest {
            upload_id: "u1".into(),
            seq: 0,
            bytes: b"first".to_vec(),
        },
        &mut uploads,
    );
    assert_eq!(first.error, None);

    let duplicate = write_chunk(
        FileWriteChunkRequest {
            upload_id: "u1".into(),
            seq: 0,
            bytes: b"duplicate".to_vec(),
        },
        &mut uploads,
    );

    assert_eq!(duplicate.bytes_written, 0);
    assert_eq!(duplicate.error, Some(FileError::InvalidSequence));
    assert!(!uploads.open.contains_key("u1"));
    assert!(!temp_path.exists());
}

#[test]
fn failed_commit_unlinks_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("payload.bin");
    let temp_path = dir.path().join("payload.bin.m80-upload.u1");

    let mut uploads = Uploads::default();
    let response = begin_upload(
        FileWriteBeginRequest {
            path: final_path.display().to_string(),
            mode: Some(0o600),
        },
        &mut uploads,
    );
    assert_eq!(response.upload_id.as_deref(), Some("u1"));

    let upload = uploads.open.get_mut("u1").unwrap();
    upload.final_path = dir.path().to_path_buf();
    let commit = commit_upload(
        FileWriteCommitRequest {
            upload_id: "u1".into(),
        },
        &mut uploads,
    );

    assert_eq!(commit.bytes_written, 0);
    assert_eq!(commit.error, Some(FileError::Io));
    assert!(!temp_path.exists());
}

fn not_found_commit() -> FileWriteCommitResponse {
    FileWriteCommitResponse {
        bytes_written: 0,
        error: Some(FileError::NotFound),
    }
}
