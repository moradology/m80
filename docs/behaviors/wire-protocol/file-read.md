# Wire Protocol — File Read

`file_read_request` reads one guest file directly in `m80-guestd`. The final
path component is opened with `O_NOFOLLOW`; symlinks return
`FileError::SymlinkRejected`. The response returns inline bytes, a `truncated`
flag, and an optional `FileError`.

Tests: `m80-proto/tests/fileops_round_trip.rs` and
`m80-guestd/tests/fileops.rs::file_read_returns_bytes`,
`file_read_honors_max_bytes`, `file_read_rejects_final_symlink`.
