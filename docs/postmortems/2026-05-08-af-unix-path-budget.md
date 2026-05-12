# 2026-05-08 — AF_UNIX path-budget regression

**Status:** structural fix + test trims merged; close pending privileged-runner
confirmation of the 6 affected tests. Tracked under `m80-o4z82`.
**Time to root cause:** ~30 min from "this list of failures looks weird" to
the `bind()` reproducer fingering the kernel cap.

## Symptom

A broader real-KVM sweep produced six failing tests. Initial reading: four
distinct regressions, sequenced unfortunately.

| Test | Reported failure |
|---|---|
| `concurrency::concurrent_stop_launch_run_dir_race` | bind: AF_UNIX path too long |
| `cgroup_memory_oom_real_kvm` | bind: AF_UNIX path too long |
| `stop_disposition_real_kvm` (1 of 4 cases) | bind: AF_UNIX path too long |
| `lifecycle_failure_real_kvm::guestd_not_ready_cold` and `restore_guestd_not_ready` | path-length launch failures |
| `lifecycle_failure_real_kvm::api_socket_timeout` and `cgroup_create_failure` | `FirecrackerPidTimeout` instead of the named target variant |
| `run_dir_invariants_real_kvm` | unexpected `ownership.lock` in run-dir top-level |
| `end_to_end_real_kvm_file_ops` | "unexpected protocol warning malformed payload" in run logs |

The dedicated `fileop_errors_real_kvm` test passed, which made the
end-to-end file-op failure look stranger still.

## What it actually was

One structural cause produced **most** of the visible failures. The other
three were independent regressions of different shapes that happened to
land in the same sweep.

### 1. Path-budget overflow (5 tests)

m80's socket path layout is

```
<run_root>/<vm_id>/<fc_basename>/<vm_id>/root/firecracker.sock
```

`vm_id` appears **twice** because m80-jailer inherits Firecracker's jailer
convention of nesting `<chroot-base>/<exec-basename>/<id>/root/`
(see `m80_jailer::jail_root_path` in `crates/m80-jailer/src/types.rs`).
With `run_root = /var/lib/m80-run` and `fc_basename = firecracker`, the path
length equals `52 + 2·len(vm_id)` bytes. The kernel's `struct sockaddr_un`
reserves 108 bytes for `sun_path` including the null terminator → 107 usable.
Any vm_id ≥ 28 chars overflows; `bind()` rejects with EINVAL "AF_UNIX path
too long".

Test helpers had been generated with full nanosecond suffixes
(`format!("{prefix}-{nanos}")`, 19+ digits), making the overflow guaranteed
for the longer-prefix tests and stochastic for the borderline ones. The
fake-firecracker tests hit the same overflow worse: the synthetic basename
`fake-firecracker-cgroup-fail-{full-nanos}` (~48 chars) consumed 33 extra
path bytes alongside vm_id.

### 2. `FirecrackerPidTimeout` in the fake-firecracker tests (independent)

`write_fake_firecracker` emitted a shell script that just slept forever:

```sh
#!/bin/sh
while true; do sleep 60; done
```

Real Firecracker writes `firecracker.pid` to its working directory after
jailer chroot+pivot. The fake didn't. m80-jailer waits 1 s for that file
(`crates/m80-jailer/src/materialized.rs:170-194`) and returns `JailerError::FirecrackerPidTimeout`
on timeout — **before** m80 even attempts the api-socket bind that the
tests asserted on. Two failure modes superimposed for the cgroup-create
case: the long synthetic basename also overflowed the path budget, but the
pid timeout fired first.

### 3. `ownership.lock` test stale (independent)

The lock's lifetime had been intentionally extended to span
launch-through-stop. The `run_dir_invariants_real_kvm.rs:81-89` assertion
declared a fixed `BTreeSet` of expected top-level entries that didn't
include `OWNERSHIP_LOCK`. Production code change had landed; the test-side
expectation hadn't.

### 4. `"malformed payload"` log substring (independent)

`m80-proto/src/error.rs:19` defines `ProtoError::MalformedPayload` with
Display `"malformed payload: {0}"`. m80-guestd's `protocol_log` emits the
Display string into `diagnostics.jsonl` on certain protocol errors. The
end-to-end test's `assert_protocol_logs_are_clean` substring-scanned both
`console.log` (the guest serial firehose) and `diagnostics.jsonl` for the
literal `"malformed payload"`. Any benign protocol-error record matching the
format triggered the assertion — no scoping to severity, phase, or
`record_protocol_error`'s canonical message prefix.

## Diagnostic path

The hypothesis-blocking move was building a direct `bind()` reproducer
**before** opening any m80 source:

```python
import socket, os
RUN_ROOT = '/var/lib/m80-run'
def test(vm_id):
    p = f'{RUN_ROOT}/{vm_id}/firecracker/{vm_id}/root/firecracker.sock'
    print(f'vm_id={vm_id!r}({len(vm_id)} chars) -> path len {len(p)}')
    os.makedirs(os.path.dirname(p), exist_ok=True)
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try: s.bind(p); print('  bind OK'); s.close(); os.remove(p)
    except OSError as e: print(f'  bind FAILED: {e}')
```

Output (against the real `/var/lib/m80-run`):

```
vm_id='persist-fs'(10)                          → path 72:  bind OK
vm_id='extract-after-cancel'(20)                → path 92:  bind OK
vm_id='cgroup-memory-oom-e2e-123456'(28)        → path 108: bind FAILED: AF_UNIX path too long
vm_id='guestd-not-ready-cold-123456789'(31)     → path 114: bind FAILED
vm_id='stop-launch-race-{full-nanos}'(36)       → path 124: bind FAILED
```

That output settled the structural cause in seconds and, more importantly,
ruled out everything else as the dominant factor. Reading
`m80-jailer::jail_root_path` afterward — `run_dir.join(exec_basename)
.join(id_basename).join("root")` — revealed the doubled vm_id.

The three independent causes fell out by reading the relevant test/source
line afterward, each in under five minutes. Those didn't need a reproducer
because the code stated the cause as soon as it was opened.

## Fix

| Cause | Fix | Where |
|---|---|---|
| Path budget structural | `ConfigError::VmIdPathBudgetExceeded` raised at `Backend::admit()` before semaphore acquisition. Pure arithmetic check via `layout::socket_path_len()` and `layout::SUN_PATH_BUDGET = 107`. | `crates/m80-firecracker/src/{backend,error,layout}.rs`, `tests/path_budget.rs`, 4 unit tests in `layout.rs` |
| Path budget tests | `unique_vm_id` helpers trimmed to `{prefix}-{:04x}` (4 hex digits, mod 0x10000). Long prefixes shortened (`api-sock-to`, `cgr-fail`, `gnr-cold`, `gnr-rest`, `fk-ambig`, `stop-unreach`, `slr`). Fake-firecracker synthetic basename trimmed from ~48 to ~16 chars. | 19 edits across `lifecycle_failure_real_kvm.rs`, `cgroup_memory_oom_real_kvm.rs`, `stop_disposition_real_kvm.rs`, `concurrency/stop_launch.rs` |
| Fake firecracker pid | Script writes `/firecracker.pid` (in-jail path → `<jail_root>/firecracker.pid` host-side) before sleeping. | `lifecycle_failure_real_kvm.rs:255` |
| `ownership.lock` test | Added `OWNERSHIP_LOCK` to the expected `BTreeSet`. | `run_dir_invariants_real_kvm.rs:81` |
| Log-assertion scoping | Parse `diagnostics.jsonl` line-by-line; match `phase == "Request"` + message prefix `"protocol error stream_id="` (the canonical `record_protocol_error` signature). Drop `console.log` from the scan. | `end_to_end_real_kvm.rs:644-673` |

The structural admission check is what prevents recurrence. The test trims
unblock the privileged battery in the meantime; under the admission check
they become defense-in-depth rather than load-bearing.

## Lessons

**Diagnostics before hypotheses extends past the host/guest boundary.** The
existing CLAUDE.md rule was framed for cross-VM debugging — make the other
side's stderr visible before guessing. The same discipline applies whenever
a behavior could come from any of several layers (kernel, m80, FC, jailer,
test scaffolding). Asking the kernel directly via a 10-line `bind()` script
cost less than reading m80's launch pipeline; doing it first replaced N
hypotheses with one observation. Had I started by reading m80 source, the
doubled-vm_id layout would have surfaced eventually but ~20 minutes later.

**Convergent failures lie about the count.** Six failing tests looked like
four problems; they were one structural cause overlapping with three
independent issues. The structural cause manifested as two different error
strings (the explicit "AF_UNIX path too long" for tests that reached
`bind()` directly, vs `FirecrackerPidTimeout` for the fake-firecracker
tests where the same path overflow short-circuited the launch sequence
earlier). Bucketing by error message would have over-counted bug classes.
Verify whether nominally distinct failures share a root before triaging them
as separate work.

**Surface caps as typed admission errors, not opaque IO.** The kernel's
108-byte `sun_path` is fixed; `bind()` returning EINVAL is the deepest
layer in the stack and the least informative. The structural fix moves that
surface up to admission as `ConfigError::VmIdPathBudgetExceeded`, naming
all four contributors (vm_id, run_root, fc_basename, computed path length).
This matches m80's "fail closed, fail clearly" posture and beats seeing a
confusing IO error during real-KVM launch.

## References

- Bead: `m80-o4z82` (epic) + 5 children (`.1` admission check, `.2` test
  trims, `.3` log scoping, `.4` ownership.lock, `.5` fake firecracker pid)
- Behavior doc: `docs/behaviors/admission/vm-id-path-budget.md`
- Code: `crates/m80-firecracker/src/layout.rs::socket_path_len`,
  `crates/m80-firecracker/src/backend.rs::check_vm_id_path_budget`,
  `crates/m80-firecracker/tests/path_budget.rs` (5 admission tests),
  `crates/m80-firecracker/src/layout.rs` `tests::socket_path_len_*` (4 unit
  tests)
