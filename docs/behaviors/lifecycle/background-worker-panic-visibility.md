# Background Worker Panic Visibility

Captured by bead `m80-wok08.3`.

m80 never treats a lifecycle or warm-pool background-thread panic as a clean
success. Panics are converted into visible diagnostics and state transitions so
operators can distinguish a normal empty pool, a timeout, and a worker that
died while it owned lifecycle state.

## Lifecycle watcher

`RunningSandbox::stop` and `RunningSandbox::force_kill` set the watcher stop
flag, then join the lifecycle watcher through `join_watcher_with_timeout`.

The join has three outcomes:

- `Joined`: no event is emitted.
- `Panicked`: m80 emits `tracing::error!` with `vm_id` and the panic payload.
- `TimedOut`: m80 emits `tracing::error!` with `vm_id` and the join timeout,
  then detaches the join waiter.

The panic payload is preserved for string payloads from both `panic!("msg")`
and `panic!("{}", msg)`. Non-string payloads are reported as
`non-string panic payload`.

## Warm-pool fill worker

The warm-pool fill worker wraps slot launch in `catch_unwind`. If slot launch
panics while a worker owns `filling > 0`, m80:

- emits `tracing::error!` with the panic payload;
- decrements `WarmPoolSnapshot::filling`;
- releases any reserved `cpuset.cpus` allocation;
- increments `fill_failures_total`, `discarded`, and
  `consecutive_fill_errors`;
- stores `last_fill_error = "panic: <payload>"`;
- notifies pool waiters and applies the normal fill-failure backoff.

This keeps `WarmPool::wait_for_idle` and `WarmPool::wait_for_ready` from
hanging or reporting a silent empty-pool condition after a worker panic.

## Evidence

- `crates/m80-firecracker/src/lifecycle/cleanup_deadline.rs` has
  `watcher_join_panic_reports_payload`.
- `crates/m80-firecracker/src/warm_pool/tests.rs` has
  `fill_worker_panic_rolls_back_filling_count`.
