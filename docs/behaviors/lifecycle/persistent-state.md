# Behavior: Persistent VM State Across Sequential Exec Calls

**Bead:** `m80-qokt.2.3`
**Date:** 2026-05-05
**Test file:** `crates/m80-firecracker/tests/persistent_state.rs`

---

## Summary

A single `RunningSandbox` serves multiple `exec` calls in sequence. Each call
runs a fresh child process inside the same VM, but the underlying filesystem
state (overlay upper layer, workspace scratch image) persists between calls.
`&mut self` on `exec` enforces the sequential constraint at compile time —
no runtime lock is needed and concurrent exec is impossible to express.

## Measured Turn-To-Turn Latency

Real-KVM bench on 2026-05-05 with the rebuilt Minimal image:

| path | N | P50 | P95 | max | source |
|---|---:|---:|---:|---:|---|
| persistent `RunningSandbox::exec` on one VM | 30 | 20.589 ms | 20.716 ms | 21.183 ms | `docs/behaviors/lifecycle/persistent-state-latency.json` |
| stock post-pivot cold launch | 30 | 1517 ms | 1518 ms | 1518 ms | `crates/m80-firecracker/benches/snapshots/2026-05-05T09:02:43+00:00.json` |

The measured turn-to-turn P50 saves about 1496 ms after the first turn
relative to a fresh cold launch. That is a roughly 74x lower steady-state
latency for stateful sessions that can keep a VM alive instead of allocating a
new sandbox for every turn.

---

## Behaviors

### B1: /tmp persists across exec calls

Files written to `/tmp` in exec N are readable in exec N+1 on the same
`RunningSandbox`. `/tmp` lives on the per-VM overlay upper layer (`rootfs.overlay.ext4`,
device `/dev/vdb`). The overlay is not reset between exec calls; it is only
discarded when the VM is stopped and the run-dir is deleted.

**Test:** `two_execs_filesystem_state_persists`
exec1 runs `touch /tmp/marker`; exec2 runs `ls /tmp/marker` and asserts
the file is present in stdout.

---

### B2: Overlay filesystem state accumulates across exec calls

Any file written anywhere in the writable overlay (not just `/tmp`) is
visible to subsequent execs. The base rootfs (`/dev/vda`) remains read-only
and shared across VMs; mutations land exclusively in the per-VM overlay upper
layer and are scoped to the lifetime of that `RunningSandbox`.

**Test:** `three_execs_increment_counter`
exec1 writes `1` to `/tmp/n`; exec2 reads it, increments to `2`, writes
back; exec3 reads, increments to `3`, writes back. The final value `3`
confirms three-turn accumulation against a single overlay.

---

### B3: Workspace scratch image persists across exec calls

When `SandboxConfig::workspace` is configured, the scratch ext4 image
(`/dev/vdc`, mounted at `/workspace` inside the VM) is shared by all exec
calls in the session. Files written to `/workspace` in exec N are visible
in exec N+1.

**Test:** `two_execs_workspace_persists`
exec1 writes `hello-workspace` to `/workspace/ws_file.txt`; exec2 reads
the file and asserts the content matches.

---

### B4: A nonzero exit from exec N does not poison the vsock channel

`ExecResponse::exit_code` is the guest process's exit status. A nonzero
exit is a property of the child process, not a protocol error. The vsock
channel between host and guestd remains intact after guestd delivers the
`ExecResponse`, regardless of exit code. Subsequent `exec` calls on the
same `RunningSandbox` succeed normally.

**Test:** `exec_after_failed_exec_still_works`
exec1 runs `/bin/false` (exits 1); `exec()` returns `Ok(response)` with a
nonzero exit code. exec2 runs `echo ok-after-fail` and asserts exit code 0
and expected stdout — confirming the channel is intact.

---

### B5: Sequential constraint is enforced at compile time

`RunningSandbox::exec` takes `&mut self`. The borrow checker prevents a
second `exec` call while the first is outstanding without any runtime
mechanism. Pipelining (concurrent exec on one VM) is not supported and
cannot be expressed in safe Rust.

This behavior has no dedicated runtime test — it is a type-system property
verified by the fact that all persistent-state tests compile and use
sequential `exec` calls without any `Mutex` or `Arc`.

---

## What does NOT persist

- **In-memory process state.** Each `ExecRequest` spawns a fresh child
  process. Environment variables set inside exec N's process are not visible
  to exec N+1's process. To pass values forward, write them to a file in the
  overlay and read them back.
- **State after stop.** Calling `stop()` (or dropping `RunningSandbox`)
  tears down the VM. The overlay is deleted when `StoppedSandbox::delete()`
  is called. There is no snapshot or checkpoint between persistent-VM sessions
  without an explicit `extract_changes` call before delete.

---

## Cross-references

- `docs/design/persistent-vm.md` §2 — authoritative design for state
  persistence guarantees.
- `docs/design/persistent-vm.md` §3 — cancellation contract (tested in
  `m80-qokt.2.4`).
- `crates/m80-firecracker/tests/end_to_end_real_kvm.rs` — single-exec
  smoke test; persistent-state tests build on its launch pattern.
