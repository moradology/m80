# Wire Protocol — File Read

`file_read_request` reads one guest file directly in `m80-guestd`. The final
path component is opened with `O_NOFOLLOW`; symlinks return
`FileError::SymlinkRejected`. The response is a stream of `file_read_chunk`
frames. Each chunk carries raw bytes in a protobuf `bytes` field, and exactly
one terminal chunk has `done=true` with the final `truncated` flag and optional
`FileError`.

Tests: `m80-proto/tests/fileops_round_trip.rs` and
`m80-guestd/tests/fileops.rs::file_read_returns_bytes`,
`file_read_honors_max_bytes`, `file_read_streams_large_file_in_multiple_chunks`,
`file_read_rejects_final_symlink`.
