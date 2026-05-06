# Stale VM Detection

## Ownership Marker

The m80 ownership marker is `<run_dir>/ownership.lock`, not predecessor's older
`ownership.json` marker. The lock records `pid=<host pid>` and
`started_at=<unix ms>`.

During recovery:

- no ownership lock means the run-dir is not actively owned
- a parseable lock whose pid is live preserves the run-dir
- a parseable lock whose pid is dead allows stale recovery to continue
- a malformed lock is ambiguous and preserves the run-dir

Tests:
- `crates/m80-firecracker/tests/concurrency/stale_detection.rs::ownership_lock_is_the_marker_file`
- `crates/m80-firecracker/tests/concurrency/stale_detection.rs::current_process_ownership_lock_preserves_run_dir`

## Health Classification

v0.1 stale recovery does not classify health by probing the API socket and
vsock socket. It first consults `ownership.lock`, then asks `m80-jailer` to
classify jailer state from files under the run-dir. Socket path existence alone
is not authority: fake or stale socket files do not preserve a run-dir after
ownership is known dead.

Test:
- `crates/m80-firecracker/tests/concurrency/stale_detection.rs::stale_detection_uses_ownership_and_jailer_state_not_socket_probes`

## Preserve On Ambiguity

Recovery is destructive only when m80 can classify the run-dir as not actively
owned and its jailer recovery state is clear enough to handle. Malformed
ownership evidence and unreadable/ambiguous jailer state are preserved for
operator inspection instead of being deleted on uncertainty.

Test:
- `crates/m80-firecracker/tests/concurrency/stale_detection.rs::preserves_run_dir_when_ownership_lock_is_ambiguous`
