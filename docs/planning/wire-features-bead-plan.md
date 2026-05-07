# Wire features: bead plan (m80-fops, m80-strm)

**Date:** 2026-05-04
**Status:** proposed; beads not yet created
**Companion docs:** `smolvm-exploration/07-public-api-and-sdk.md`, `smolvm-exploration/08-agent-semantics-and-use-cases.md`
**Scope reminder:** per `CLAUDE.md`, m80 is generic VM mechanics. No tool-catalog, no policy, no path-prefix gates. Both epics are additive on the existing length-prefixed JSON wire and the existing `Envelope<serde_json::Value>` peek-and-dispatch in `m80-guestd::connection::handle_connection` (`crates/m80-guestd/src/connection.rs:73`). Neither epic introduces a new crate; types land in `m80-proto`, handlers in `m80-guestd`, host wrappers in `m80-firecracker`.

---

## Epic 1: `m80-fops` — file-ops verbs (read / write / list / stat / chunked-write)

```
id: m80-fops
title: File-ops verbs on the guest wire (read / write / list / stat / chunked write)
status: open
priority: 2
labels: [agent, wire-protocol, active-v0.2]
dependencies: []
```

**Description**

Today every "read a file out of the VM" or "drop a file into the VM" round-trips through a `bash -c`-style `ExecRequest` with base64-encoded stdin and stdout. That works but it is expensive (process spawn + pipe), is bounded by the 1 MiB capture cap in `connection::CAPTURE_LIMIT`, and forces every caller to construct shell-quoted command lines. m80-fops adds first-class file-ops verbs to the wire — `FileRead`, `FileWrite`, `FileList`, `FileStat`, and a chunked-upload trio (`FileWriteBegin`, `FileWriteChunk`, `FileWriteCommit`) for files that do not fit in a single envelope. The verbs run in m80-guestd directly: open the path, read or write bytes, return them. No subprocess, no shell.

This exists because m80's primary consumer is agent harnesses, and agents constantly move small files (configs, snippets, patches, log tails) in and out of the sandbox. Smolvm's protocol has shipped equivalents (`FileRead`, `FileWrite`, `FileWriteBegin`, `FileWriteChunk` per `smolvm-protocol/src/lib.rs`); the gap is real, not speculative. The chunked variant matters specifically for "agent uploads a 100 MB tarball into the workspace before running a build" — a single-envelope `FileWrite` would have to fit in one length-prefixed JSON frame, which is a poor model for that case.

**Why now.** Wire protocol has stabilised behind `Envelope::kind` peek-and-dispatch. Each new verb is a `Payload` impl plus a handler arm — no framing changes, no version bump, no migration story. Adding the verbs after v0.2 means writing two callers to the awkward bash path before flipping them.

**Non-goals.**
- No path-prefix policy ("only `/workspace` is writable"). m80 is generic VM mechanics; the caller decides paths. A m80-adapter can layer policy on top.
- No symlink-following on read. We `O_NOFOLLOW` the final component and reject (return `Failed`) if the target is a symlink. Documented; not configurable in v0.2.
- No permissions / chown verbs. `mode` is honoured on `FileWrite` create; no separate `chmod`/`chown` API. Callers that need it use `ExecRequest`.
- No streaming reads. `FileRead` returns the whole body in one envelope (subject to `max_bytes`). For tailing a growing file, use `m80-strm` with `tail -f`.
- No directory-tree creation in `FileWrite`. The parent directory must already exist; per CLAUDE.md "no silent recovery" we surface the missing-dir error rather than mkdir-p'ing.
- No checksumming on the wire. The existing JSON+base64 framing is checked end-to-end by the JSON parser; a corrupted body will fail to deserialize.

---

### Leaf 1: DESIGN — lock the verb set, error model, chunked-upload state machine

```
id: m80-fops.1
title: DESIGN — file-ops verb set, error model, chunked upload protocol
status: open
priority: 2
labels: [agent, wire-protocol, design, active-v0.2]
dependencies: [m80-fops]
```

**Description.** Pin the contract every IMPL leaf consumes:

1. **Verb set (final).** Five "small" verbs — `FileRead`, `FileWrite`, `FileList`, `FileStat`, `FileRemove` — plus a chunked-upload trio (`FileWriteBegin` → 0..N `FileWriteChunk` → `FileWriteCommit`). No `FileMove`, no `FileCopy` in v0.2; out of scope.
2. **Error model.** Every verb returns its dedicated response type which carries an `Option<FileError>` discriminant: `NotFound`, `PermissionDenied`, `IsADirectory`, `NotADirectory`, `SymlinkRejected`, `TooLarge`, `InvalidSequence`, or `Io`. Per CLAUDE.md "no catch-and-rewrap" — the `Io` variant is reserved for cases that don't map to a domain variant; we do *not* shadow `NotFound` as `Io`. Reclassification is uniform across the surface.
3. **Symlinks.** Final component is opened with `O_NOFOLLOW`; intermediate components follow the kernel default (the alternative — `O_PATH` walk + `openat2` with `RESOLVE_NO_SYMLINKS` — is overkill for v0.2). A symlink at the final component yields `SymlinkRejected`.
4. **Size caps.** `FileRead` honours an optional `max_bytes` (default: a `FILE_READ_LIMIT` constant in `m80-proto`, proposed 16 MiB). When the file is larger than `max_bytes` we read `max_bytes` and set `truncated: true`. `FileWrite`'s body is implicitly capped by the active encoded protobuf frame cap (currently 4 MiB); larger uploads must use the chunked path.
5. **Chunked upload state machine.** `FileWriteBegin { path, mode }` returns `FileWriteBeginResponse { upload_id }`. The guest creates `<path>.m80-upload.<upload_id>` and keeps an open file handle keyed by `upload_id` in a per-connection map. Each `FileWriteChunk { upload_id, seq, bytes }` appends only when `seq` matches the next expected zero-based sequence. `FileWriteCommit { upload_id }` fsyncs, atomically renames `<path>.m80-upload.<upload_id>` → `<path>`, and drops the handle. Disconnect on the connection unconditionally drops the handle and unlinks the temp file (no resume across reconnects in v0.2). The guest rejects `FileWriteChunk` for sequence gaps or repeats with `InvalidSequence` and `FileWriteChunk`/`Commit` for an unknown id with `NotFound`.
6. **Listing.** `FileList { path }` reads one directory level (no recursion). `DirEntry { name, kind: FileKind, size }` where `FileKind` is `File | Dir | Symlink | Other`. `size` for directories is the raw `st_size` (not the recursive total); document that.
7. **Stat.** `FileStat` returns `kind`, `size`, `mtime_unix_ms`, `mode` (Unix mode bits as `u32`). No `inode`, `device`, `ctime`, or `xattrs` in v0.2.
8. **Concurrency.** Each request is handled inline on the connection thread, same as `ExecRequest`. No background work, no separate vsock channel.

**Acceptance.**
- Doc landed at `docs/design/wire-fops.md` with verb table, error matrix, and chunked-upload sequence diagram.
- `crates/m80-proto/README.md` "Public surface" updated with the new payload types in the same diff.
- No code in this leaf.

**Effort.** S (3–4 hours).

---

### Leaf 2: IMPL m80-proto — payload types + `Payload` impls

```
id: m80-fops.2
title: IMPL m80-proto — file-ops payload types and Payload impls
status: open
priority: 2
labels: [agent, wire-protocol, active-v0.2]
dependencies: [m80-fops.1]
```

**Description.** Add the 13 new payload types and their `Payload::KIND` constants to `crates/m80-proto/src/types.rs`. Re-export from `crates/m80-proto/src/lib.rs`. No framing changes.

Wire snippet (paste-ready):

```rust
// New PAYLOAD_KIND_ constants in m80-proto/src/types.rs

pub const PAYLOAD_KIND_FILE_READ_REQUEST: &str        = "file_read_request";
pub const PAYLOAD_KIND_FILE_READ_RESPONSE: &str       = "file_read_response";
pub const PAYLOAD_KIND_FILE_WRITE_REQUEST: &str       = "file_write_request";
pub const PAYLOAD_KIND_FILE_WRITE_RESPONSE: &str      = "file_write_response";
pub const PAYLOAD_KIND_FILE_LIST_REQUEST: &str        = "file_list_request";
pub const PAYLOAD_KIND_FILE_LIST_RESPONSE: &str       = "file_list_response";
pub const PAYLOAD_KIND_FILE_STAT_REQUEST: &str        = "file_stat_request";
pub const PAYLOAD_KIND_FILE_STAT_RESPONSE: &str       = "file_stat_response";
pub const PAYLOAD_KIND_FILE_REMOVE_REQUEST: &str      = "file_remove_request";
pub const PAYLOAD_KIND_FILE_REMOVE_RESPONSE: &str     = "file_remove_response";
pub const PAYLOAD_KIND_FILE_WRITE_BEGIN_REQUEST: &str = "file_write_begin_request";
pub const PAYLOAD_KIND_FILE_WRITE_BEGIN_RESPONSE: &str= "file_write_begin_response";
pub const PAYLOAD_KIND_FILE_WRITE_CHUNK_REQUEST: &str = "file_write_chunk_request";
pub const PAYLOAD_KIND_FILE_WRITE_CHUNK_RESPONSE: &str= "file_write_chunk_response";
pub const PAYLOAD_KIND_FILE_WRITE_COMMIT_REQUEST: &str= "file_write_commit_request";
pub const PAYLOAD_KIND_FILE_WRITE_COMMIT_RESPONSE: &str = "file_write_commit_response";

/// Default cap for `FileReadRequest::max_bytes` when caller passes `None`.
pub const FILE_READ_LIMIT_DEFAULT: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind { File, Dir, Symlink, Other }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileError {
    NotFound,
    PermissionDenied,
    IsADirectory,
    NotADirectory,
    SymlinkRejected,
    TooLarge,
    InvalidSequence,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileReadRequest {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileReadResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
    /// Empty when `error` is `Some`.
    #[serde(with = "b64::single")]
    pub bytes: Vec<u8>,
    /// True iff `bytes.len() == max_bytes` and there is more data on disk.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileWriteRequest {
    pub path: String,
    #[serde(with = "b64::single")]
    pub bytes: Vec<u8>,
    /// Unix mode bits used when the file is being created. Ignored on
    /// existing files. `None` -> 0o644.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileWriteResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
    pub bytes_written: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileListRequest { pub path: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirEntry {
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileListResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
    pub entries: Vec<DirEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileStatRequest { pub path: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileStatResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
    pub kind: FileKind,
    pub size: u64,
    pub mtime_unix_ms: u64,
    pub mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRemoveRequest { pub path: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRemoveResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
}

// --- Chunked upload trio ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileWriteBeginRequest {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileWriteBeginResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
    /// Opaque token bound to this connection. UUID-v4 in the guest impl.
    pub upload_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileWriteChunkRequest {
    pub upload_id: String,
    #[serde(with = "b64::single")]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileWriteChunkResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
    pub bytes_written_total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileWriteCommitRequest { pub upload_id: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileWriteCommitResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
    pub bytes_written_total: u64,
}

// Plus 16 `impl Payload for ... { const KIND = PAYLOAD_KIND_...; }` blocks.
```

**Acceptance.**
- All types, KIND constants, and `Payload` impls in `crates/m80-proto/src/types.rs`.
- Round-trip serde tests (`#[test]` per request/response pair, no bundling) — touch `crates/m80-proto/src/types.rs:251` test mod.
- File stays under the 500-line guideline; if `types.rs` would cross it, split per the CLAUDE.md submodule rule (e.g. `types/exec.rs`, `types/fops.rs`, `types/shutdown.rs`).
- `crates/m80-proto/README.md` updated.

**Effort.** M (1 day).

---

### Leaf 3: IMPL m80-guestd — handler arms + per-connection upload table

```
id: m80-fops.3
title: IMPL m80-guestd — file-ops handlers and per-connection upload state
status: open
priority: 2
labels: [agent, wire-protocol, active-v0.2]
dependencies: [m80-fops.2]
```

**Description.** In `crates/m80-guestd/src/connection.rs`, extend the dispatch in `handle_connection` (currently the `match raw.kind.as_str()` at line 73) with arms for each new `PAYLOAD_KIND_FILE_*`. Add a new module `crates/m80-guestd/src/fops.rs` with one handler fn per verb plus a `UploadTable` struct (`HashMap<String, UploadHandle>`) owned by the connection.

Per-verb handlers:
- `handle_file_read`: `OpenOptions::new().read(true).custom_flags(O_NOFOLLOW).open(path)`. On `ELOOP` return `SymlinkRejected`. Read up to `max_bytes`; if reader returns more bytes available afterwards, set `truncated: true`.
- `handle_file_write`: `OpenOptions::new().write(true).create(true).truncate(true).mode(mode.unwrap_or(0o644)).open(path)`, write all bytes, fsync.
- `handle_file_list`: `read_dir`, map each entry to `DirEntry`. `kind` derived from `file_type()`.
- `handle_file_stat`: `lstat` (so the symlink itself is reported, not its target). Convert `mtime` ns → ms.
- `handle_file_remove`: `unlink` for files; `rmdir` for empty dirs (verb returns `IsADirectory` only when caller targets a non-empty dir). Use `lstat` first to pick the correct call.
- Chunked verbs use the per-connection `UploadTable`.

Connection lifecycle: change `handle_connection`'s outer signature so the upload table lives across requests *on the same connection* — but in v0.1 each connection handles exactly one envelope and returns. Per smolvm-exploration/08, that keeps things simple, but breaks chunked uploads. So either (a) make `handle_connection` loop until the peer hangs up, or (b) require the chunked-upload trio to live within one envelope each but persist on a new "upload session" channel. Option (a) is the smaller change and matches the smolvm model; pick (a) and document that exec/file-ops requests are now multiplexed serially on one connection. The accept loop in `m80-guestd::main` is unchanged.

On disconnect (read error / EOF), drop the entire `UploadTable`; each `UploadHandle::Drop` unlinks its temp file.

**Acceptance.**
- File-ops handlers in `crates/m80-guestd/src/fops.rs` (split from `connection.rs` per file-size rule).
- `handle_connection` loops until EOF; documented in module-level rustdoc.
- Unit tests with `Cursor`-backed reader/writer for happy paths and the documented error variants. One `#[test]` per scenario (no bundling).
- Integration-style test that uploads payloads larger than the active frame cap across multiple chunks and verifies the byte-for-byte content + fsync semantics.
- `crates/m80-guestd/README.md` updated.

**Effort.** L (3–4 days).

---

### Leaf 4: IMPL m80-firecracker — host-side wrapper API on `RunningSandbox`

```
id: m80-fops.4
title: IMPL m80-firecracker — RunningSandbox file-ops methods
status: open
priority: 2
labels: [agent, wire-protocol, active-v0.2]
dependencies: [m80-fops.3]
```

**Description.** Add to `crates/m80-firecracker/src/lifecycle.rs` (alongside `RunningSandbox::exec` at line 47): `read_file`, `write_file`, `list_dir`, `stat`, `remove`, plus a chunked-upload helper `upload_file(&mut self, path: &str, source: impl Read)` that internally drives Begin/Chunk/Commit with a configurable chunk size (default 4 MiB). Each method does one envelope round-trip on `self.channel`, mirroring `exec`'s send/recv pattern. Errors from the guest (`Some(FileError)`) map to a new `FcError::File(FileError)` variant.

**Acceptance.**
- Six methods on `RunningSandbox`. `phase_event` calls per round-trip, same as `exec_send`/`exec_recv`.
- `FcError::File(FileError)` variant added with `thiserror` mapping; existing `FcError` variants untouched.
- `crates/m80-firecracker/README.md` "Public surface" updated.
- Loopback integration test under `crates/m80-firecracker/tests/fops/` boots a VM, writes an inline file plus a chunked file larger than the active frame cap, reads them back, lists and stats them, removes them.

**Effort.** M (1–2 days).

---

### Leaf 5: TESTS — wire round-trips, error matrix, chunked-upload edge cases

```
id: m80-fops.5
title: TESTS — file-ops wire round-trips and error matrix
status: open
priority: 2
labels: [agent, wire-protocol, test, active-v0.2]
dependencies: [m80-fops.3]
```

**Description.** Per-leaf tests are colocated; this leaf is the cross-cutting matrix:
- Each `FileError` variant produced by each verb has its own `#[test]` (no bundling). Symlink-at-final-component → `SymlinkRejected`. Read on a directory → `IsADirectory`. Write into a non-existent parent → `NotFound` via uniform errno mapping.
- `FileRead` truncation: write 17 MiB, `FileRead { max_bytes: 16 MiB }`, expect `bytes.len() == 16 MiB && truncated == true`.
- Chunked upload: disconnect mid-`FileWriteChunk`, verify `<path>.m80-upload.<id>` is unlinked and `<path>` does not exist.
- Chunked upload: two concurrent `upload_id`s on one connection writing different files — both commit cleanly.
- `FileWriteChunk` with unknown `upload_id` → `NotFound`.
- `FileWriteChunk` with a sequence gap or duplicate → `InvalidSequence`.

**Acceptance.** Tests live next to the code they exercise (`crates/m80-guestd/tests/fops/`, `crates/m80-proto/src/types/fops.rs::tests`). All scenarios in distinct `#[test]` fns.

**Effort.** M (1 day).

---

### Leaf 6: DOCS — behavior captures + adapter migration note

```
id: m80-fops.6
title: DOCS — fops behaviors and exec-bash deprecation note for adapters
status: open
priority: 3
labels: [agent, wire-protocol, docs, active-v0.2]
dependencies: [m80-fops.4]
```

**Description.** One behavior doc per verb under `docs/behaviors/fops/<verb>.md` (six total + one for the chunked trio). Each pins the present-tense fact: "the daemon, given X, returns Y."  Plus `docs/migration/fops-vs-exec-bash.md`: a short note for the future m80-adapter explaining when to use `read_file` vs `exec("cat …")` (short answer: always prefer the verb except when shell expansion is required).

**Acceptance.** Seven behavior docs + one migration doc. README cross-links updated.

**Effort.** S (half a day).

---

## Epic 2: `m80-strm` — real streaming exec

```
id: m80-strm
title: Streaming exec on the guest wire (multi-frame ExecStdout/Stderr/Exit)
status: open
priority: 2
labels: [agent, wire-protocol, active-v0.2]
dependencies: []
```

**Description**

Today m80-guestd buffers stdout and stderr into 1 MiB `Vec<u8>` via `capture_stream` and returns one `ExecResponse` envelope at process exit (`crates/m80-guestd/src/connection.rs:213-215`, `:271-292`). For agents this is wrong on two axes: first, long-running commands (a build, a test suite, a `cargo run` server probe) emit useful chunks of output minutes before they exit, and the agent's UX collapses if all of it arrives at once at the end; second, the 1 MiB cap silently drops everything past the limit. m80-strm reframes the exec wire as a stream of envelopes — `ExecStdout { bytes }` and `ExecStderr { bytes }` frames pushed as the child writes them, terminated by exactly one `ExecExit { status, exit_code, timing }` frame. The existing batched `ExecRequest` keeps working: callers receive the same `ExecResponse` shape via a host-side convenience that buffers internally on top of the streaming wire.

This is an unclaimed differentiator. Smolvm advertises streaming but its wire collects events into a `Vec` before returning (per `smolvm-exploration/07-public-api-and-sdk.md`); kata, libkrun, and e2b all buffer at the agent boundary. Real streaming on a fast vsock channel costs us very little engineering and is genuinely visible to anyone running an agent harness side-by-side.

**Why now.** The wire's framing is already length-prefixed JSON (`m80-proto::framing`). A request_id binds many response frames to one request, which is exactly the mechanic the existing exec uses to ignore (it sets request_id once, gets one response). We do not need a session protocol or a sub-channel — the precedent is `Envelope<serde_json::Value>` peek-and-dispatch on the guest side (`connection.rs:73`, used today for shutdown vs exec); the same peek on the *host* side covers stdout-vs-stderr-vs-exit.

**Non-goals.**
- No bidirectional stdin streaming. `ExecRequest::stdin` stays a single-shot byte buffer in v0.2. A future epic can add `ExecStdin { bytes }` push frames if there is demand; right now no caller wants it.
- No multiplexed concurrent execs on one channel. One exec request at a time; pipelining stays explicitly unsupported.
- No PTY allocation. Streaming over piped stdout/stderr only. `tput` / colour / cursor-control behaviour is unchanged from v0.1.
- No frame compression. JSON+base64 is the same on the wire as v0.1; a slow exec emitting 100 MiB of output produces 100 MiB of base64 over vsock. Acceptable for v0.2.
- No persisted-stream resume across reconnects.
- No structured event types (`exec_started`, `exec_killed`). One terminal frame; that is `ExecExit`.

---

### Leaf 1: DESIGN — multi-frame wire shape, terminal frame, cancellation, backpressure

```
id: m80-strm.1
title: DESIGN — streaming exec wire, terminal frame, cancellation, backpressure
status: open
priority: 2
labels: [agent, wire-protocol, design, active-v0.2]
dependencies: [m80-strm]
```

**Description.** Pin the contract:

1. **Frame shape.** Three new payload types: `ExecStdout { bytes: Vec<u8> }`, `ExecStderr { bytes: Vec<u8> }`, `ExecExit { status: ExecStatus, exit_code: Option<i32>, truncated: Option<bool>, timing: ExecTiming }`. All three are wrapped in `Envelope<T>` with the same `request_id` as the originating `ExecRequest`. Order is the order the guest produced the bytes, modulo stdout-vs-stderr interleaving (we make no global ordering guarantee between the two streams, since they run on separate threads — document this).
2. **Terminal frame.** Exactly one `ExecExit` per `ExecRequest`. Reading `ExecExit` is the host's signal to stop reading frames for that request. No `ExecStdout` or `ExecStderr` may follow it on the wire.
3. **Trigger.** The guest opts into streaming when the request is `ExecRequest` and the host has signalled it understands streaming. We add `ExecRequest::streaming: bool` (default false for backward compat). When `streaming == false` the guest's behaviour is exactly v0.1 — one `ExecResponse` envelope. When `streaming == true` the guest emits the new frame sequence.
4. **Batched-API convenience.** `RunningSandbox::exec` keeps the same signature; it sets `streaming = true` internally, reads frames off the channel, concatenates `ExecStdout` into `stdout: Vec<u8>` and `ExecStderr` into `stderr: Vec<u8>`, and synthesizes an `ExecResponse` from the `ExecExit` frame. Callers see no behavioural change. The streaming entry point is a new method, `exec_streaming(req, on_chunk)`.
5. **Cancellation on host disconnect.** The guest's existing exec already kills the child if the connection drops because `child.wait` is followed by a `write_frame` that fails on a closed channel. With streaming we now write *while* the child is alive, so a write failure mid-stream is observable immediately. On any write error from `write_frame` for an `ExecStdout` / `ExecStderr` frame, the guest calls `child.kill()`, drains, and exits the handler. Document: "Host disconnect during a streaming exec kills the child within one chunk (~milliseconds)."
6. **Backpressure.** vsock's underlying `AF_VSOCK` socket buffer caps the in-flight bytes (default 256 KiB on Linux). When the host falls behind, `write_frame` blocks; the guest's stdout-capture thread blocks pushing into the writer; the kernel's pipe buffer fills; the child's `write(2)` blocks. End-to-end: a slow host *throttles* the guest child rather than dropping bytes. Document this as the explicit semantic. No lossy buffer in front of `write_frame`.
7. **Capture cap behaviour.** The 1 MiB `CAPTURE_LIMIT` is *removed* in the streaming path. Truncation cannot happen because there is no in-guest buffer. `ExecExit::truncated` is `None` for streaming responses. The batched-API convenience caps host-side buffering at the same default 1 MiB and reports `truncated = Some(true)` on overflow, preserving v0.1 callers' expectation. (Document this asymmetry.)
8. **Chunk size.** The guest's stdout-capture thread reads up to 32 KiB at a time and emits one `ExecStdout` per non-empty read. No batching, no time-based flushing. Latency-optimal for terminal-style output.

**Acceptance.** Doc at `docs/design/wire-streaming.md`. `crates/m80-proto/README.md` and `crates/m80-firecracker/README.md` updated.

**Effort.** S–M (half day to a day).

---

### Leaf 2: IMPL m80-proto — streaming payload types

```
id: m80-strm.2
title: IMPL m80-proto — ExecStdout/ExecStderr/ExecExit and ExecRequest::streaming
status: open
priority: 2
labels: [agent, wire-protocol, active-v0.2]
dependencies: [m80-strm.1]
```

Wire snippet:

```rust
pub const PAYLOAD_KIND_EXEC_STDOUT: &str = "exec_stdout";
pub const PAYLOAD_KIND_EXEC_STDERR: &str = "exec_stderr";
pub const PAYLOAD_KIND_EXEC_EXIT:   &str = "exec_exit";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecStdout {
    #[serde(with = "b64::single")]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecStderr {
    #[serde(with = "b64::single")]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecExit {
    pub status: ExecStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    pub timing: ExecTiming,
}

impl Payload for ExecStdout { const KIND: &'static str = PAYLOAD_KIND_EXEC_STDOUT; }
impl Payload for ExecStderr { const KIND: &'static str = PAYLOAD_KIND_EXEC_STDERR; }
impl Payload for ExecExit   { const KIND: &'static str = PAYLOAD_KIND_EXEC_EXIT; }

// Add one field on ExecRequest. Default false preserves v0.1 wire bytes:
// when `streaming == false` it serializes nothing (Default + skip_if).
//
// In ExecRequest:
#[serde(default, skip_serializing_if = "is_false")]
pub streaming: bool,

fn is_false(b: &bool) -> bool { !*b }
```

**Acceptance.** Round-trip serde tests for each new type. Test that `ExecRequest { streaming: false, .. }` produces wire bytes byte-identical to a v0.1 `ExecRequest` (no `streaming` field on the wire). README updated. File-size budget respected.

**Effort.** S (half day).

---

### Leaf 3: IMPL m80-guestd — streaming capture path

```
id: m80-strm.3
title: IMPL m80-guestd — streaming exec capture and write
status: open
priority: 2
labels: [agent, wire-protocol, active-v0.2]
dependencies: [m80-strm.2]
```

**Description.** In `crates/m80-guestd/src/connection.rs`, branch in `handle_exec` on `req.streaming`:
- `false` → existing `exec_request` path (unchanged).
- `true` → new `exec_request_streaming` path: spawn child as today, but instead of `capture_stream` collecting into `Vec<u8>`, each capture thread loops reading up to 32 KiB and pushing into a bounded `mpsc::sync_channel` of `(Stream, Vec<u8>)` events. The main thread drains the channel and calls `write_frame(writer, &Envelope::with_request_id(ExecStdout { bytes }, request_id))` per event. Stream tagging: `enum Stream { Stdout, Stderr }`. When both readers EOF, `wait_with_timeout` runs (unchanged), then one `ExecExit` frame is written.
- Write failure on any frame → `child.kill()`, drain remaining events to `/dev/null`, return `Continue` without further writes.
- The connection loop (added in `m80-fops.3`) keeps the channel open after `ExecExit` for the next request, mirroring file-ops semantics.

This removes `CAPTURE_LIMIT` from the streaming path. `ExecExit::truncated` is always `None` here.

**Acceptance.** Streaming branch in `connection.rs`. Tests: streaming path with a `Cursor` writer that lets us inspect individual frames in order; verify N stdout frames + 1 exit frame for `printf hello | head` style; verify kill-on-write-failure with a writer that errors after the second frame. Each scenario its own `#[test]`.

**Effort.** L (2–3 days).

---

### Leaf 4: IMPL m80-firecracker — `exec_streaming` + buffered `exec` rebuild

```
id: m80-strm.4
title: IMPL m80-firecracker — exec_streaming + batched exec rebuilt on streaming
status: open
priority: 2
labels: [agent, wire-protocol, active-v0.2]
dependencies: [m80-strm.3]
```

**Description.** Two host changes in `crates/m80-firecracker/src/lifecycle.rs`:

1. New method `RunningSandbox::exec_streaming<F>(&mut self, req: ExecRequest, on_chunk: F) -> Result<ExecExit, FcError> where F: FnMut(ExecChunk)` with `pub enum ExecChunk<'a> { Stdout(&'a [u8]), Stderr(&'a [u8]) }` defined in the same module (or in `m80-firecracker::types`). The method sets `req.streaming = true`, sends one envelope, then loops: peek `Envelope<serde_json::Value>::kind`; on `exec_stdout` decode and pass `Stdout(&bytes)`; on `exec_stderr` pass `Stderr(&bytes)`; on `exec_exit` decode and return.
2. Reimplement `RunningSandbox::exec` (line 47) on top of `exec_streaming`: maintain two `Vec<u8>` host-side, append from the closure, cap each at `HOST_BUFFER_LIMIT` (1 MiB to match v0.1). On exit, build an `ExecResponse` from the `ExecExit` plus the buffered bytes, setting `truncated = Some(true)` if either buffer hit its cap. v0.1 callers see no signature or behavioural change.

`FcError` gains `FcError::ExecFraming(String)` for the case where a frame arrives with an unexpected `kind` between `ExecRequest` send and `ExecExit`.

**Acceptance.** Both methods on `RunningSandbox`. Loopback integration test runs `bash -c "for i in $(seq 1 100); do echo line $i; sleep 0.01; done"` and verifies the closure observes ~100 stdout chunks across the run, not all at the end. (Use a 50 ms wall-clock dispersion check: time of first chunk < time of last chunk by at least 500 ms.) Existing batched-`exec` tests pass unchanged. README updated.

**Effort.** M (1–2 days).

---

### Leaf 5: TESTS — backpressure, cancellation, terminal-frame invariant

```
id: m80-strm.5
title: TESTS — streaming backpressure, cancellation, terminal-frame invariant
status: open
priority: 2
labels: [agent, wire-protocol, test, active-v0.2]
dependencies: [m80-strm.4]
```

**Description.** Cross-cutting tests; one `#[test]` per scenario:

- Terminal-frame invariant: exactly one `ExecExit`; no `ExecStdout`/`ExecStderr` follows it.
- Order invariant within a single stream: stdout chunks arrive in write order. Cross-stream order *not* guaranteed; assert only same-stream order.
- Cancellation: host drops the channel mid-stream, run a guest-side probe (e.g. `bash -c "trap '' TERM; sleep 60"` followed by signal-survival check) and verify the child is killed via `SIGKILL` within 100 ms. Confirms the documented cancellation behaviour.
- Backpressure: host reads slowly (sleep between frame reads); guest produces a 100 MB stream from `cat /dev/urandom | head -c 100M`; assert end-to-end success and that wall-clock time on the guest scales with host read rate (proves no in-guest unbounded buffer). Use a `<NOT_FLAKY_TOLERANCE>` margin documented in the test.
- Truncation asymmetry: `exec` (batched) on a 2 MiB stdout returns `truncated = Some(true)`; `exec_streaming` on the same command sees all 2 MiB of chunks and returns `ExecExit::truncated = None`.
- Backwards-compat: `ExecRequest { streaming: false }` over a guest built from this leaf produces the exact `ExecResponse` shape and bytes that v0.1 produced.

**Acceptance.** All scenarios distinct `#[test]` fns under `crates/m80-firecracker/tests/streaming/` and `crates/m80-guestd/tests/streaming/`.

**Effort.** M (1–2 days).

---

### Leaf 6: DOCS — behaviors + cancellation + backpressure narrative

```
id: m80-strm.6
title: DOCS — streaming exec behaviors and cancellation/backpressure narrative
status: open
priority: 3
labels: [agent, wire-protocol, docs, active-v0.2]
dependencies: [m80-strm.4]
```

**Description.** Behavior docs:
- `docs/behaviors/exec/streaming-frames.md` — pins the multi-frame wire shape.
- `docs/behaviors/exec/streaming-cancellation.md` — pins "host disconnect kills child via write-failure detection within one chunk."
- `docs/behaviors/exec/streaming-backpressure.md` — pins "vsock socket buffer + pipe buffer is the only buffer; slow host throttles guest child."
- `docs/behaviors/exec/streaming-terminal-frame.md` — pins "exactly one `ExecExit`."
- `docs/behaviors/exec/batched-on-streaming.md` — pins "batched `exec()` is implemented on `exec_streaming` and caps host-buffered stdout/stderr at 1 MiB each."

Each doc has a corresponding test under `crates/m80-firecracker/tests/streaming/<topic>.rs` per the CLAUDE.md "doc + test per leaf" rule.

**Acceptance.** Five behavior docs + cross-links from the two affected READMEs.

**Effort.** S (half day).

---

## Cross-epic notes

- **No new crate.** Both epics live entirely in `m80-proto` (types), `m80-guestd` (handlers), and `m80-firecracker` (host wrappers). Per CLAUDE.md "no junk drawers."
- **Wire compatibility.** `m80-fops` adds new `kind`s; an old guest seeing one returns `Failed` (already true via the catch-all arm at `connection.rs:76`). `m80-strm` adds an opt-in field to `ExecRequest` defaulted to `false`; old guest sees no field and the existing `exec_request` path runs unchanged. Neither epic requires a `PROTOCOL_VERSION` bump.
- **Connection-loop change is shared.** `m80-fops.3` introduces the connection-loop semantic (multiple envelopes per accept). `m80-strm` depends on this only insofar as the ergonomic of leaving the connection open for the next exec is convenient — a streaming exec by itself fits in one accept. Sequence the two epics so `m80-fops.3` lands first; `m80-strm.3` then uses the same loop pattern.
- **File-size budget.** `m80-proto/src/types.rs` will exceed 500 lines after these payloads; split into `types/exec.rs`, `types/fops.rs`, `types/shutdown.rs`, `types/handshake.rs` per the CLAUDE.md submodule rule. `m80-guestd/src/connection.rs` similarly splits into `connection.rs` + `exec.rs` + `fops.rs` + `streaming.rs`.
- **What is *not* here.** No tool catalog, no `tool_call_id`, no policy-gated path prefixes, no idempotency keys, no semantic events. Those belong in the future m80-adapter per CLAUDE.md scope-boundary.

---

## Sequencing

```
m80-fops.1  ─┐
             ├─> m80-fops.2 ─> m80-fops.3 ─> m80-fops.4 ─> m80-fops.5,6
             │
m80-strm.1  ─┴─> m80-strm.2 ─> m80-strm.3 ─> m80-strm.4 ─> m80-strm.5,6
                                      ^
                                      │ depends on connection-loop semantic
                                      │ introduced in m80-fops.3
```

m80-fops and m80-strm DESIGN leaves can run in parallel. IMPL on streaming waits for the connection-loop change in `m80-fops.3`. Both epics target v0.2.
