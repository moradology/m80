# RunningSandbox Drop Order

Behavior capture for bead `m80-xeaey.1`.

## Contract

`RunningSandbox` field order is load-bearing because Rust drops struct fields
in declaration order. The implicit-drop path is the fallback for callers that
abandon a running VM without calling `stop()` or `force_kill()`.

The required order is:

1. `kill_guard` signals the watcher, force-kills Firecracker/jailer, and
   unmounts the snapshot bind.
2. `watcher_thread` is detached only after the watcher has been told to stop.
3. `cgroup` kills and removes the leaf after the VM process is dead.
4. `jail` tears down the chroot after process and cgroup cleanup.

Happy-path `stop()` and explicit `force_kill()` disarm `kill_guard` before
their own teardown. This invariant protects the abandoned-handle path only.

## Verification

- `crates/m80-firecracker/src/types.rs::tests::running_sandbox_field_order_keeps_implicit_drop_fail_closed`
- `crates/m80-firecracker/tests/stop_disposition_real_kvm.rs::implicit_drop_force_kills_before_resource_teardown`
- Real-KVM smoke: `./scripts/smoke.sh launch-only`
