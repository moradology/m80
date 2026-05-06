# Wire Protocol — File Write

`file_write_request` creates or truncates one guest file without spawning a
process. The parent directory must already exist. The final component uses
`O_NOFOLLOW`; optional Unix mode is applied after the write.

Tests: `m80-proto/tests/fileops_round_trip.rs` and
`m80-guestd/tests/fileops.rs::file_write_creates_file_with_mode`.
