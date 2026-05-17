# Drop Cleanup Diagnostics

Captured by bead `m80-xeaey.3`.

RAII cleanup paths are best-effort because `Drop` must not panic, but cleanup
failures must still be visible. m80 logs cleanup failures instead of discarding
them silently in the following paths:

- `WarmLease::drop` and `WarmPool::drop` log `discard_sandbox` failures so a
  possible leaked VM is visible.
- `LeaseGuard::drop` logs non-`NotFound` run-root lock removal failures so a
  stale lock is diagnosable.
- `TemplateLock::drop` logs non-`NotFound` template-lock removal failures.
- guestd upload-table drop logs abandoned temp-file removal failures.
- CLI signal watcher drop logs a panicked watcher thread.

The behavior remains fail-closed for lifecycle authority: logging a `Drop`
failure does not pretend cleanup succeeded, and non-drop cleanup APIs still
return typed errors to the caller.

Evidence:

- `crates/m80-firecracker/src/warm_pool/lease.rs`
- `crates/m80-firecracker/src/warm_pool.rs`
- `crates/m80-firecracker/src/runroot.rs`
- `crates/m80-storage/src/rootfs.rs`
- `crates/m80-guestd/src/connection/fileops.rs`
- `crates/m80-cli/src/cmds/signal_watcher.rs`
