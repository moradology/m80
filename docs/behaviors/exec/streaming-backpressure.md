# Streaming Exec Backpressure

Guestd uses a bounded one-frame handoff from each capture thread to the
connection writer. This is intentionally small: slow host reads must throttle
the guest child through the pipe/socket path, not grow an unbounded in-guest
buffer.

Pressure chain:

```
host stops reading
  -> host-side receive buffer fills
  -> guest-side send blocks
  -> one-frame capture handoff fills
  -> stdout/stderr capture thread blocks
  -> child pipe fills
  -> child blocks in write(2)
```

Buffered `RunningSandbox::exec` is built on top of streaming exec and keeps
the old 1 MiB cap per stream. When that cap is reached, it keeps draining the
stream through `ExecExit` while dropping extra buffered bytes and setting
`ExecResponse::truncated = Some(true)`.

Tests:

- `crates/m80-guestd/tests/streaming_exec.rs::streaming_chunk_write_failure_kills_child_promptly`
- `crates/m80-firecracker/src/lifecycle/exec.rs::tests::append_capped_reports_truncation_after_limit`

