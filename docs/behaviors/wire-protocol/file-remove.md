# Wire Protocol — File Remove

`file_remove_request` removes one non-directory guest path. Missing paths
return `FileError::NotFound`; directories return `FileError::IsADirectory`.

Tests: `m80-proto/tests/fileops_round_trip.rs` and
`m80-guestd/tests/fileops.rs::file_remove_deletes_file`,
`file_remove_rejects_directory`.
