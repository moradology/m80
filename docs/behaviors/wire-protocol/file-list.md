# Wire Protocol — File List

`file_list_request` lists one directory level and returns
`DirEntry { name, kind, size }`. It does not recurse. Entry metadata uses
symlink metadata so symlink entries are reported as `FileKind::Symlink`.

Tests: `m80-proto/tests/fileops_round_trip.rs` and
`m80-guestd/tests/fileops.rs::file_list_reports_symlink_kind_without_following`.
