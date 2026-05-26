# Wire Protocol — Chunked Upload

Chunked upload uses `file_write_begin_request`, one or more
`file_write_chunk_request` frames, and `file_write_commit_request` on the same
connection. Guestd stores open uploads in a per-connection table and writes to
`<path>.m80-upload.<upload_id>` until commit fsyncs and renames the file.
Disconnect before commit drops the table and unlinks temp files.

Chunk sequences start at zero and advance by one for each upload id. Guestd
rejects gaps and duplicates with `FileError::InvalidSequence`, removes the
failed upload, unlinks the temp file before appending bytes, writes the error
ack, and closes the upload connection. The host fails closed if a chunk ack
echoes the wrong upload id or sequence.

Tests: `m80-proto/tests/fileops_round_trip.rs::chunked_upload_payloads_round_trip`,
`m80-guestd/src/connection/fileops/tests.rs::chunked_upload_rejects_sequence_gap_without_writing`,
and `m80-firecracker/src/lifecycle/fileops.rs::tests::chunk_ack_rejects_wrong_sequence`.
