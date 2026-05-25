# Run-Root Recovery Loop

## Spawn

`m80-firecracker` v0.1 does not spawn a background run-root recovery task from
`Backend::new`, `Backend::admit`, or `Sandbox::launch`. `Backend::new` runs one
synchronous startup recovery pass, and callers may still invoke
`Backend::recover_stale_run_root()` explicitly at later orchestration
boundaries.

This intentionally differs from predecessor's service-owned periodic loop. m80
is a generic library/CLI surface; hidden background recovery would make
ownership and timing harder for embedders to reason about.

Test:
- `crates/m80-firecracker/tests/concurrency/recovery_loop.rs::backend_new_runs_one_startup_recovery_pass`

## Interval

There is no m80-firecracker recovery interval constant in v0.1. A resident
owner or future adapter may choose a cadence, but the library surface remains a
one-shot recovery method.

Test:
- `crates/m80-firecracker/tests/concurrency/recovery_loop.rs::recovery_interval_is_not_an_orchestrator_constant_in_v0_1`

## Blocking Task

Recovery is synchronous and filesystem-bound. If a tokio service wants to call
it without occupying an async worker, that service must use its own
`spawn_blocking` or equivalent. m80-firecracker does not spawn runtime tasks.

Test:
- `crates/m80-firecracker/tests/concurrency/recovery_loop.rs::recovery_is_synchronous_explicit_call`

## Startup Pass

Startup recovery runs once during `Backend::new`. This pass is synchronous.
Most per-run-dir cleanup failures are logged and construction continues, but a
stale cgroup leaf with live pids is surfaced as `FcError::StaleCgroupLeaf` by
the recovery API and logged by `Backend::new` while preserving the run-dir. A
CLI/admin command or resident owner can still run
`Backend::recover_stale_run_root()` later and receive the typed error.

Test:
- `crates/m80-firecracker/tests/concurrency/recovery_loop.rs::explicit_recovery_api_remains_available_after_startup_pass`

## Mid-Launch Race

A caller-owned recovery loop may run while new VMs are launching. Recovery must
preserve fresh run directories whose `ownership.lock` still belongs to a live
m80 process; otherwise a cleanup pass can kill a VM between run-root creation
and ready.

Test:
- `crates/m80-firecracker/tests/concurrency/recovery_loop.rs::recovery_during_launch_preserves_fresh_vms`
  (#[ignore]) runs `recover_stale_run_root()` in a 10 ms loop while ten real
  KVM launches race through admit, launch, exec, stop, and delete.
