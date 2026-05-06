# Wire Streaming Exec

**Bead:** `m80-5vha.1`
**Status:** DESIGN - no code landed in this leaf

---

## 1. Goal

Buffered exec is useful for short commands, but it is the wrong surface for a
thin process wrapper. Long builds, test runs, `tail -f`, and text user
interfaces need stdout and stderr as the child produces them.

Streaming exec keeps the existing length-prefixed JSON envelope protocol and
adds one opt-in mode:

```rust
pub struct ExecRequest {
    // existing fields...
    #[serde(default, skip_serializing_if = "is_false")]
    pub streaming: bool,
}
```

`streaming == false` is the default and serializes identically to the v0.1
request shape. `streaming == true` changes only the response side: the guest
sends zero or more output chunk envelopes followed by one terminal envelope.

This is a hard cutover design for the v0.x workspace. Host and guest move
together; there is no compatibility shim.

---

## 2. Payloads

### 2.1 New payload kinds

```
exec_stdout
exec_stderr
exec_exit
```

### 2.2 Output chunks

```rust
pub struct ExecStdout {
    pub seq: u32,
    #[serde(with = "b64::single")]
    pub bytes: Vec<u8>,
}

pub struct ExecStderr {
    pub seq: u32,
    #[serde(with = "b64::single")]
    pub bytes: Vec<u8>,
}
```

`seq` is monotonic per stream within one request. Stdout and stderr each start
at `0`; their sequence numbers are independent. Ordering between streams is the
order in which frames are written to the connection, not a total timestamped
ordering created by the protocol.

The `request_id` lives on the enclosing `Envelope`, as it does for
`ExecRequest` and `ExecResponse`. Chunk payloads do not duplicate it.

### 2.3 Terminal frame

```rust
pub struct ExecExit {
    pub status: ExecStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub total_stdout_bytes: u64,
    pub total_stderr_bytes: u64,
    pub truncated: bool,
    pub timing: ExecTiming,
}
```

`ExecExit` is the streaming equivalent of `ExecResponse` without inline
stdout/stderr buffers.

`truncated` is `false` for the normal streaming path. It is present so the
terminal frame can represent future guest-side caps without changing shape.
The buffered host wrapper still applies the existing 1 MiB-per-stream cap when
it rebuilds an `ExecResponse` from chunks; that wrapper sets
`ExecResponse::truncated` when its local buffer cap is reached.

---

## 3. Frame Timeline

### 3.1 Successful command with mixed output

```
host  -> guest  Envelope<ExecRequest> { kind=exec_request, request_id=R, streaming=true }
guest -> host   Envelope<ExecStdout>  { kind=exec_stdout,  request_id=R, seq=0, bytes="a\n" }
guest -> host   Envelope<ExecStderr>  { kind=exec_stderr,  request_id=R, seq=0, bytes="warn\n" }
guest -> host   Envelope<ExecStdout>  { kind=exec_stdout,  request_id=R, seq=1, bytes="b\n" }
guest -> host   Envelope<ExecExit>    { kind=exec_exit,    request_id=R, status=completed,
                                        exit_code=0, total_stdout_bytes=4,
                                        total_stderr_bytes=5, truncated=false, timing=... }
```

The terminal frame is final. After `ExecExit`, the guest handler returns to the
accept loop. No `ExecStdout` or `ExecStderr` frame for `R` may appear after
`ExecExit`.

### 3.2 Command with no output

```
host  -> guest  Envelope<ExecRequest> { request_id=R, streaming=true }
guest -> host   Envelope<ExecExit>    { request_id=R, status=completed, exit_code=0,
                                        total_stdout_bytes=0, total_stderr_bytes=0,
                                        truncated=false, timing=... }
```

Zero chunks is valid. Exactly one terminal frame is still required.

### 3.3 Guest spawn failure

Spawn failure is terminal, not a chunk:

```
host  -> guest  Envelope<ExecRequest> { request_id=R, streaming=true }
guest -> host   Envelope<ExecExit>    { request_id=R, status=failed, exit_code=None,
                                        total_stdout_bytes=0, total_stderr_bytes=N,
                                        truncated=false, timing=... }
```

The failure detail remains stderr-shaped process output. If the failure detail
is emitted before terminal construction, guestd may send it as `ExecStderr`
chunks and report the byte total in `ExecExit`. If the failure happens before
the streaming capture path exists, guestd may include no chunks and report
`status=failed`. Implementations must be consistent inside one response.

---

## 4. Guest Write Model

The guest keeps the current two-reader shape:

1. Spawn the child with stdout/stderr piped.
2. Start one capture thread per pipe.
3. Each capture thread reads bounded chunks, normally 4 KiB raw bytes.
4. Each capture thread serializes `Envelope<ExecStdout>` or
   `Envelope<ExecStderr>` through a shared writer lock.
5. The exec coordinator waits for the child and both capture threads.
6. After both streams are drained, it writes exactly one `Envelope<ExecExit>`.

The writer lock is a serialization point only. It must not become an unbounded
queue. A slow host should backpressure the capture thread directly.

Chunks must remain well below `MAX_FRAME_BYTES` after base64 and JSON encoding.
The proposed 4 KiB raw read size is deliberately conservative.

---

## 5. Backpressure

Streaming exec must not accumulate an unbounded in-guest buffer.

The intended pressure chain is:

```
host stops reading
  -> host-side socket receive buffer fills
  -> guest socket send buffer fills
  -> guest capture thread blocks while writing a chunk frame
  -> child's stdout/stderr pipe fills
  -> child blocks in write(2)
```

That behavior is acceptable and desired. It gives the caller control over
throughput without a separate rate-limit protocol.

The implementation must avoid:

- collecting all chunks into a `Vec` before sending;
- pushing chunks into an unbounded channel between capture threads and writer;
- spawning a detached writer that can outlive the exec request;
- treating host slowness as output truncation.

Buffered `exec()` is rebuilt on top of `exec_streaming()` host-side. It buffers
stdout and stderr up to the existing 1 MiB cap per stream, drops excess bytes,
continues draining the streaming protocol, and returns a normal `ExecResponse`.
That drain is important: the host must continue reading through `ExecExit` even
after the local buffer cap is reached, or it will turn a large-output command
into artificial guest backpressure.

---

## 6. Cancellation FSM

The existing `CancelRequest`, `CancelAck`, and `CancelStatus` envelope types
are reused exactly. Streaming exec does not add a second cancellation protocol.

### 6.1 Explicit cancel

```
Running
  host sends Envelope<CancelRequest> with matching request_id
  guest sends SIGKILL to child
  guest reaps child and capture threads
  guest writes Envelope<CancelAck> { status=cancelled }
  guest writes no ExecExit for that request
  handler returns to accept loop
```

`CancelAck::AlreadyExited` and `CancelAck::Failed` keep their existing meanings.

### 6.2 Caller disconnect

A streaming caller may disappear without sending `CancelRequest`. The guest
must treat connection loss as cancellation of the in-flight child.

There are two detection paths:

1. **Write-side failure:** writing a chunk or terminal frame returns an I/O
   error such as `EPIPE`. The guest kills and reaps the child, then returns to
   the accept loop.
2. **Read-side EOF:** a read watcher observes EOF on the same connection while
   the child is still running. The guest kills and reaps the child, then
   returns to the accept loop.

The read-side path is required. Write-side `EPIPE` alone is insufficient
because a silent command such as `sleep 600` may produce no chunks for minutes.
Dropping the host-side stream must not leave that process running until its
natural exit.

### 6.3 Timeout

`ExecRequest::timeout_ms` keeps the v0.1 meaning. If the timeout fires first,
guestd kills the child and sends `ExecExit { status=timed_out, ... }` after
draining any already-captured output. No `CancelAck` is sent for a timeout.

### 6.4 Terminal invariants

For a request that reaches normal completion or timeout:

- exactly one `ExecExit` is sent;
- `ExecExit` is sent after stdout/stderr capture threads are drained;
- no chunks are sent after `ExecExit`.

For explicit cancel or disconnect:

- `CancelAck` may be sent for explicit cancel;
- no `ExecExit` is sent;
- the child is killed and reaped before the handler returns.

---

## 7. Host API Contract

`m80-firecracker` exposes two surfaces:

```rust
pub enum ExecChunk {
    Stdout { seq: u32, bytes: Vec<u8> },
    Stderr { seq: u32, bytes: Vec<u8> },
}

pub fn exec_streaming(
    &mut self,
    req: ExecRequest,
    on_chunk: impl FnMut(ExecChunk) -> Result<(), FcError>,
) -> Result<ExecExit, FcError>;

pub fn exec(&mut self, req: ExecRequest) -> Result<ExecResponse, FcError>;
```

`exec_streaming` sets `req.streaming = true`, sends the request, calls
`on_chunk` for each stdout/stderr frame in wire order, and returns the terminal
`ExecExit`. If `on_chunk` returns an error, the host drops the streaming
connection and returns that error; guestd observes the disconnect and cancels
the in-flight child.

`exec` is the buffered convenience wrapper. It uses `exec_streaming`, appends
chunk bytes into per-stream buffers capped at 1 MiB, and constructs the
existing `ExecResponse` from those buffers and the returned `ExecExit`.

The public API stays synchronous. No async runtime is introduced in the
foundation crates.

---

## 8. Non-Goals

- No stdin streaming. `ExecRequest::stdin` remains fire-and-forget bytes sent
  with the request.
- No PTY allocation. Terminal mode is a separate CLI/guest epic.
- No concurrent execs on one connection.
- No request multiplexing. One connection carries one exec request and its
  response stream.
- No compression.
- No resume across reconnects.
- No tool semantics, path policy, workspace identity, or effect classes.

---

## 9. Tests Required By Implementation Leaves

Protocol tests:

- `ExecRequest { streaming: false }` serializes byte-identically to the v0.1
  golden request.
- `ExecRequest { streaming: true }` includes `streaming: true`.
- `ExecStdout`, `ExecStderr`, and `ExecExit` round-trip through
  `Envelope<T>`.

Guestd unit tests:

- stdout chunks have monotonic per-stream sequence numbers;
- stderr chunks have monotonic per-stream sequence numbers;
- mixed stdout/stderr output ends with exactly one `ExecExit`;
- write failure kills and reaps the child;
- read-side EOF kills a silent child;
- explicit `CancelRequest` kills the child and returns `CancelAck`.

Host/firecracker tests:

- `exec()` and `exec_streaming()` produce equivalent bytes for the same command;
- large output sets the buffered `ExecResponse::truncated` flag while streaming
  continues to drain through `ExecExit`;
- a full-VM ignored test observes chunk arrival over time, not only after exit;
- a full-VM ignored test drops the streaming caller during a silent long-running
  command and observes the child reaped promptly.
