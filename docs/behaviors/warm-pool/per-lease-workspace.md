# Per-Lease Workspace Attach

Captured by beads `m80-r4308.15` and `m80-r4308.16`.

Warm-pool slots boot stateless. `WarmPool::new` rejects a
`SandboxConfig::workspace` because a ready slot must be reusable for any
caller until it is leased. A caller that needs per-workload storage attaches it
after `WarmPool::try_lease` returns a `WarmLease`.

The per-lease workspace path is a preallocated drive-hotplug slot:

1. The pool's `SandboxConfig::preallocated_drive_slots` reserves one or more
   placeholder Firecracker drives before `InstanceStart`.
2. The caller creates or selects a tenant/workspace image visible inside the
   Firecracker jail.
3. `WarmLease::attach_drive_verified(HotplugDriveAttach)` retargets the slot
   with Firecracker `PATCH /drives/{id}`, asks guestd to mount it, and verifies
   opaque identity bytes from the mounted filesystem before returning success.
4. `WarmLease::detach_drive(HotplugDriveDetach)` asks guestd to unmount the
   path and retargets the slot back to its placeholder file.

Attach, detach, protocol, mount, and identity failures consume the running slot.
The lease releases the failed slot and starts pool refill, so callers never get
back a VM with uncertain tenant-drive state.

`WarmLease` also forwards typed file operations to the leased `RunningSandbox`:
`read_file`, `write_file`, `list_dir`, `stat_file`, `create_dir`,
`remove_file`, and `upload_file_chunked`. Those operations keep
`FcError::FileOp` detail and avoid shell-quoting guest paths or payloads.
File operations and drive attach/detach do not consume a one-shot lease's
workload token; exec and PTY remain the workload boundary.

Evidence:

- `crates/m80-firecracker/src/warm_pool/lease.rs` exposes the WarmLease
  file-operation and drive attach/detach facade.
- `crates/m80-firecracker/tests/bestiary_stand_in_real_kvm.rs` exercises
  attach identity verification, typed file operations through `WarmLease`,
  out-of-range slot rejection, and attach-detach-attach reuse of one slot.
- `crates/m80-firecracker/tests/warm_pool.rs` exercises dead ready-slot
  rejection, runtime target resize, and warm-pool counters.
