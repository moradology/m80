# Wire Protocol — File Stat

`file_stat_request` stats one guest path with symlink metadata and returns
`FileStat { kind, size, mtime_unix_ms, mode }`. It does not expose inode,
device, ctime, or xattrs.

Tests: `m80-proto/tests/fileops_round_trip.rs` and
`m80-guestd/tests/fileops.rs::file_stat_returns_mode_and_kind`.
