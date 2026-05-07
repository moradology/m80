# Deferred Storage Readiness Decision

Behavior bead: `m80-3xwa.5.3`.

## Decision

m80-guestd does not signal host readiness before PID-1 storage setup completes.
The G2 proposal to use a `OnceLock`-style deferred storage mount is rejected for
the current PID-1 architecture.

Readiness remains:

1. PID-1 stdio redirection and panic hook.
2. `/proc`, `/sys`, and `/dev` mounts.
3. `/dev/vda` lower mount, `/dev/vdb` upper mount, overlayfs mount, and
   `pivot_root` into the merged root.
4. Optional `/dev/vdc` workspace mount when `m80.workspace=1`.
5. Vsock listener bind.
6. Inverted ready signal to the host.

## Why

The storage setup is not only an optional data mount. In minimal PID-1 mode,
`crates/m80-guestd/src/pid_one.rs` makes the mount namespace private, mounts the
read-only base rootfs at `/lower`, mounts the per-VM writable ext4 at `/upper`,
mounts overlayfs at `/merged`, bind-mounts pseudo filesystems into `/merged`,
and then pivots into `/merged`.

If guestd signaled readiness before that point, host requests could observe the
wrong root. `ExecRequest`, PTY, file ops, metrics paths that inspect procfs, and
workspace operations would run against pre-pivot state instead of the VM
contract described by `crates/m80-guestd/README.md` and
`docs/behaviors/lifecycle/preboot-wiring.md`.

Deferring only the workspace mount is also not a safe default. The host boot
args explicitly communicate `m80.workspace=1`; after readiness, callers can send
commands with `cwd=/workspace`, file-op requests under `/workspace`, or drive
detach requests that assume the mount is present. A first-use mount would add a
new request-time failure mode and a hidden serialization point to otherwise
simple guest verbs.

The smolvm-style `OnceLock` pattern is useful when storage is an optional
service dependency. In m80 PID-1 mode, storage defines the guest-visible
filesystem contract. Treating it as lazy initialization would trade a possible
cold-start latency win for semantic drift at the host/guest boundary.

## Replacement Direction

Cold fresh launch performance should be improved by measuring and optimizing the
existing phases, not by publishing readiness before the filesystem contract is
true. Acceptable follow-up work includes:

- timing each PID-1 storage phase in `M80_GUEST_BOOT` milestones;
- reducing mount or fsck time for `/dev/vdb` and `/dev/vdc`;
- snapshotting after storage setup so warm slots start after the expensive
  rootfs path;
- making optional workspace fallback explicit before readiness, not lazy after
  readiness.

The readiness invariant is: once the host accepts the ready signal, guestd can
serve any supported request against the final rootfs and configured workspace
view.
