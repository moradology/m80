# CLI Pipe Streaming

Behavior capture for bead `m80-lt15.19`.

## Contract

Normal `m80 run` pipe mode streams guest output as the guest process writes it.
It does not wait for process exit before copying stdout or stderr.

Rules:

- guest stdout chunks are written to host stdout immediately;
- guest stderr chunks are written to host stderr immediately;
- stdout and stderr stay separated;
- wrapper diagnostics never appear on stdout;
- the `m80` process exits with the guest exit code after the terminal exec frame;
- global `--json` is not pipe-transparent and keeps using the buffered
  structured exec response.

The CLI consumes `RunningSandbox::exec_streaming` for pipe mode. JSON mode uses
`RunningSandbox::exec` because its contract is a serialized response object, not
foreground process I/O.

## Ordering

The streaming wire preserves the frame order guestd observes while copying
stdout and stderr. The CLI writes each frame to its matching host stream and
flushes that stream before processing the next frame. m80 does not synthesize a
global timestamp order and does not merge stderr into stdout.

## Size

Pipe mode is not capped by the buffered `ExecResponse` 1 MiB per-stream limit.
Large output is streamed through stdout/stderr until the guest exits or the host
side fails. The old cap remains only on `RunningSandbox::exec` and therefore on
`m80 --json run`.

## Backpressure And Cancellation

The CLI's output writes are part of the backpressure chain. If the host stops
reading, stdout/stderr writes eventually block, which blocks the vsock receive
path and then guestd's bounded capture path.

If a host stdout/stderr write fails, `exec_streaming` returns that I/O error.
The CLI closes the streaming connection and force-stops the sandbox through the
same wrapper-error path used by other exec failures. guestd treats the dropped
connection as cancellation for the in-flight child.

Signal and process-tree semantics are tracked separately in
`m80-lt15.22` and `m80-lt15.22.1`.

## Tests

- `crates/m80-cli/src/cmds/tests.rs::run_passthrough_copies_streaming_chunks_without_crossing_streams`
- `crates/m80-cli/tests/e2e_run_passthrough.rs::stdout_chunk_arrives_before_guest_process_exits` (ignored real-KVM)
- `crates/m80-cli/tests/e2e_run_passthrough.rs::pipe_streaming_stdout_is_not_capped_at_one_mib` (ignored real-KVM)
