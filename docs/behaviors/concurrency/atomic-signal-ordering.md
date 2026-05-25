# Atomic Signal Ordering

Cross-thread boolean signals in `m80-firecracker` use Release/Acquire ordering:

- lifecycle watcher publication flags (`idle_timed_out`, `lifetime_expired`);
- watcher shutdown flags (`watcher_stop` / `stop_flag`);
- exec and PTY forwarder stop flags;
- warm-pool shutdown and target-ready resize publication.

The publishing thread stores with `Release`; the consuming thread loads with
`Acquire` before deciding whether to accept work, exit a loop, keep or discard a
slot, or resize fill behavior.

Counters and monotonic timestamps remain `Relaxed` because they do not publish
non-atomic state:

- `active_execs`
- `last_activity_ns`
- warm-pool `next_slot`

Tests:

- `crates/m80-firecracker/src/lifecycle.rs::tests::idle_watcher_does_not_timeout_while_exec_active`
- `crates/m80-firecracker/src/lifecycle.rs::tests::lifecycle_watcher_idle_can_win_before_longer_lifetime`
- `crates/m80-firecracker/src/lifecycle.rs::tests::lifecycle_watcher_lifetime_can_win_before_longer_idle_timeout`
- `crates/m80-firecracker/src/warm_pool/tests.rs` exercises shutdown and fill-worker signaling paths.
