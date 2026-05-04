# `m80-firecracker`

The orchestrator. Composes the foundation crates into a usable sandbox:
preflight → boot → ready → run → stop → cleanup. Owns the lifecycle
state machine, the run-root layout, the admission semaphore, and the
configuration loading order.

## Reason for being

The foundation crates are leaves with crisp contracts; this is the one
place they compose. Without it every consumer of m80 would assemble the
lifecycle by hand — exactly what makes Firecracker hard to use out of
the box.

## Black-box contract

### Lifecycle state machine

```
Created → Running → Stopped → (Deleted | preserved-for-triage)
```

Each state is a distinct Rust type (`Sandbox`, `RunningSandbox`,
`StoppedSandbox`); transitions consume the prior handle so callers can't
double-stop or exec on a stopped VM. `force_kill` collapses Running →
Stopped while preserving the run-dir for offline inspection.

The Created → Running transition runs the strict 12-phase preboot pipeline
internally. Phases are sub-steps, not states — failures at any phase
return a typed `FcError` and drop the admission permit.

### Run-root layout

All per-VM state lives under `<run_root>/<vm_id>/`. The actual jailer
chroot is at `<run_root>/<vm_id>/<exec basename>/<vm_id>/root/` (jailer's
hardcoded layout — see `m80-jailer`).
`Backend::recover_stale_run_root()` is a one-shot orchestrator-driven
scan; v0.1 has no background recovery thread. Cross-process collision
avoidance: distinct `<run_root>` paths.

### Concurrency / admission

`Backend::admit().launch()` acquires one slot from the admission
semaphore (sized by `M80_MAX_CONCURRENT_VMS`, default 4). The permit is
held for the lifetime of the sandbox and dropped on `delete()`.

### Configuration

Loading order: built-in defaults → `/etc/m80/config.toml` →
`~/.config/m80/config.toml` → `M80_*` env → CLI flags. Reveal the merged
result via `Backend::show_effective_config()`.

### Error model

Typed `FcError` variants tell the caller which phase failed; inner
causes carry detail. No silent degradation — anything that compromises
an invariant fails closed.

## Non-goals

- **No agent semantics.** No tool catalog, no `EffectClass`, no authority
  leases. m80's job is "boot a VM and run a command".
- **No multi-host placement.** Single host only.
- **No persistent VM pools.** Every `launch()` is a fresh boot in v0.1;
  warm pools are a v0.2 epic.
- **No "execute and forget".** All sandboxes return through `stop()` or
  `force_kill()`.
