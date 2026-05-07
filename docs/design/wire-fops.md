# Wire File Operations

## Verb Table

| Request | Response | Notes |
|---|---|---|
| `FileReadRequest { path, max_bytes }` | `FileReadResponse { bytes, truncated, error }` | Reads one file with final-component `O_NOFOLLOW`. Default cap is `FILE_READ_LIMIT_DEFAULT` (16 MiB). |
| `FileWriteRequest { path, bytes, mode }` | `FileWriteResponse { bytes_written, error }` | Creates/truncates one file. Parent directory must exist. Inline writes are bounded by the active encoded protobuf frame cap before guestd dispatch. |
| `FileListRequest { path }` | `FileListResponse { entries, error }` | Lists one directory level, no recursion. |
| `FileStatRequest { path }` | `FileStatResponse { stat, error }` | Uses symlink metadata; reports symlink kind instead of following it. |
| `FileRemoveRequest { path }` | `FileRemoveResponse { removed, error }` | Removes one non-directory path. |
| `FileWriteBeginRequest { path, mode }` | `FileWriteBeginResponse { upload_id, error }` | Creates `<path>.m80-upload.<upload_id>` in a per-connection upload table. |
| `FileWriteChunkRequest { upload_id, seq, bytes }` | `FileWriteChunkResponse { upload_id, seq, bytes_written, error }` | Appends bytes to the open upload and acks each chunk. |
| `FileWriteCommitRequest { upload_id }` | `FileWriteCommitResponse { bytes_written, error }` | `fsync`s, applies mode, renames temp to final path, and syncs before close. |

## Error Matrix

Responses carry `Option<FileError>`:

| Error | Meaning |
|---|---|
| `NotFound` | Target path, parent path, or upload id does not exist. |
| `PermissionDenied` | Guest kernel denied the operation. |
| `IsADirectory` | Operation required a non-directory file. |
| `NotADirectory` | Operation required a directory in the path. |
| `SymlinkRejected` | Final component was a symlink where the verb rejects following it. |
| `TooLarge` | Transfer accounting exceeded a bounded verb's size limit. Oversized inline frames normally fail at protobuf framing before guestd dispatch. |
| `InvalidSequence` | Chunked upload request sequence did not match the next expected chunk. |
| `Io` | Unmapped I/O failure. |

Path-prefix policy is not in m80. Callers decide which paths are acceptable
before sending these verbs.

## Chunked Upload

The host opens one vsock connection, sends `FileWriteBeginRequest`, receives an
`upload_id`, sends one or more chunk frames, then sends commit. Upload state is
per connection. If the connection drops before commit, guestd drops the upload
table and removes temporary files. There is no resume across reconnects.

Chunk sequences start at zero and advance by one within an upload id. Guestd
rejects gaps and duplicates with `InvalidSequence`, removes the failed upload,
and unlinks its temp file without appending bytes. The host also verifies each
chunk ack echoes the active upload id and expected sequence before it sends the
next chunk.
