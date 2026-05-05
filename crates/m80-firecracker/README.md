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

### Drive layout

Firecracker assigns block-device names in PUT order: first PUT becomes
`/dev/vda`, second `/dev/vdb`, etc. m80 PUTs in this order:

| Position | drive_id          | Host file                                | RO/RW   | Guest path | Purpose |
|---------:|-------------------|------------------------------------------|---------|------------|---------|
|        1 | `rootfs`          | `<image>/output.ext4` (shared base)      | **RO**  | `/dev/vda` | Read-only base ext4. Bind-mounted into the jail at `/rootfs.ext4`. Same host file for every VM that uses this image — host page cache deduplicates. |
|        2 | `rootfs_overlay`  | `<run_dir>/rootfs.overlay.ext4`          | RW      | `/dev/vdb` | Per-VM sparse ext4. m80-guestd's PID-1 setup mounts `/dev/vda` as the lowerdir, this as the upperdir, overlayfs on `/`. |
|        3 | `workspace`       | `<run_dir>/scratch.ext4` (if requested)  | RW      | `/dev/vdc` | Per-VM workspace ext4. Mounted at `/workspace`. Subject to opt-in `Scratch::extract` after stop. Only present when `SandboxConfig::workspace_dir.is_some()`. |

The base + overlay split is what `m80-storage::Rootfs` produces;
`m80-storage::Scratch` is the workspace. Per-VM sparse files cost ~10 ms
each at most to allocate + format; there is no full-rootfs copy.

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
- **No persistent VM pools.** `RunningSandbox::exec` takes `&mut self` and
  the VM stays alive between sequential exec calls; warm-pool snapshotting
  (boot once, restore many) is a separate v0.2 epic (`m80-rrp`).
- **No "execute and forget".** All sandboxes return through `stop()` or
  `force_kill()`.
