# Wire Protocol — Chunked Upload

Chunked upload uses `file_write_begin_request`, one or more
`file_write_chunk_request` frames, and `file_write_commit_request` on the same
connection. Guestd stores open uploads in a per-connection table and writes to
`<path>.m80-upload.<upload_id>` until commit fsyncs and renames the file.
Disconnect before commit drops the table and unlinks temp files.

Tests: `m80-proto/tests/fileops_round_trip.rs::chunked_upload_payloads_round_trip`
and `m80-guestd/tests/fileops.rs::chunked_upload_writes_chunks_and_commits`.
