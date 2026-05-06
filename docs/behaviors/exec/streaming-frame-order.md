# Streaming Exec Frame Order

When `ExecRequest::streaming` is true, guestd emits zero or more
`ExecStdout` / `ExecStderr` frames followed by exactly one `ExecExit`.

Frame rules:

- all frames carry the request's `Envelope::request_id`;
- stdout sequence numbers are monotonic within stdout;
- stderr sequence numbers are monotonic within stderr;
- stdout/stderr interleaving is the writer-observed order, not a synthetic
  timestamp order;
- `ExecExit` is written only after both capture threads have drained;
- no stdout/stderr frame may follow `ExecExit`.

Tests:

- `crates/m80-proto/tests/streaming_round_trip.rs`
- `crates/m80-guestd/tests/streaming_exec.rs::streaming_stdout_stderr_chunks_end_with_exit`
- `crates/m80-guestd/tests/streaming_exec.rs::streaming_no_output_still_sends_terminal_exit`

