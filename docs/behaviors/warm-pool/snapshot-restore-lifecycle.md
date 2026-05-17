# Snapshot Restore Lifecycle

This behavior instantiates the template layer from
[`docs/decisions/0007-snapshot-template-lifecycle.md`](../../decisions/0007-snapshot-template-lifecycle.md)
on top of the raw Firecracker restore contract in
[`docs/design/snapshot-restore.md`](../../design/snapshot-restore.md).

Snapshot-template warm pools use the existing one-use warm-lease model. A
template-backed slot is restored during fill work, not during
`WarmPool::try_lease`. The checkout path remains allocation-only: it pops a
ready slot, verifies the recorded Firecracker process is still alive, records
the lease, and starts normal refill.

## Fill Sequence

`WarmStrategy::SnapshotRestore` fill runs this sequence:

1. Compute live `TemplateInputs` from current preflight discovery, the
   stateless `SandboxConfig`, and the configured `HookSpecSet`.
2. Ask `m80-snapshot-template` for the matching template. A miss invokes the
   producer path; a hit returns a process-local `PinnedTemplate`.
3. Resolve any declared pmem layers through `m80-image-store` and bind their
   stable jail-visible backings (`/pmem.<slot>.img`) before restore.
4. Bind the committed template body directory read-only at `/snapshot` after
   rejecting symlinked body paths.
5. Load and resume the Firecracker snapshot.
6. Probe the restored guestd exec channel.
7. If hooks are configured, send one `PostRestoreHookRequest`, wait up to the
   fixed 5 second aggregate response deadline for all hook results, and fail
   closed on timeout or the first typed hook error.
8. Run the configured ready probe.
9. Push the slot into the ready queue only after the preceding steps succeed.

No half-restored or partially hook-initialized template VM is visible through
`WarmPool::try_lease`. A failure in lookup, build, restore, hook dispatch, or
ready probing records a fill failure and leaves the ready queue unchanged.
Successful fills retain a bounded sequence of duration samples that measurement
harnesses can drain with `WarmPool::take_fill_duration_samples_us`.

Template restore uses `m80_snapshot::restore_preverified` after the
content-addressed template store has committed and pinned the template body.
Direct caller-supplied snapshots still use normal `m80_snapshot::restore`,
which rehashes the snapshot pair before every load.

## Lease Boundary

Warm-pool leases are one-use slots. `WarmLease::Drop` and
`WarmLease::discard` force-kill and delete the leased VM, release any cpuset
range, record the lease return, and trigger background refill. Since a
template-backed ready slot is leased at most once, fill-time restore is the
per-slot and per-lease freshness boundary for v0.1.

A future caller that needs checkout-specific hook arguments, such as a hostname
chosen after pool fill, must get a separate API and tests. It must not
silently move the existing `SnapshotRestore` strategy into a second restore
path inside `try_lease`.

## Evidence

- `crates/m80-firecracker/src/warm_pool/inner.rs` contains the fill-time
  `WarmStrategy::SnapshotRestore` sequence.
- `crates/m80-firecracker/src/warm_pool/template_build/tests.rs` contains
  `template_lookup_builds_miss_once_then_hits_cache`.
- `crates/m80-firecracker/src/warm_pool/tests.rs` contains
  `snapshot_restore_fingerprint_mismatch_records_fill_failure`.
- `crates/m80-firecracker/src/warm_pool/template_build/tests/real_kvm.rs`
  contains `snapshot_restore_warm_strategy_fills_ready_slot`.
- `crates/m80-firecracker/src/lifecycle/post_restore.rs` contains unit tests
  for ordered hook conversion, response validation, and typed hook failure
  mapping.
- `crates/m80-firecracker/benches/snapshot_template_restore_latency.rs`
  measures restore-to-handback latency for `m80-q420k.4.15`; the verified
  artifact is documented in
  `docs/perf/snapshot-template-restore.md`.
