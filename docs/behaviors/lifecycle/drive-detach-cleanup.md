# Drive Detach Cleanup

Captured by bead `m80-iswt.7`.

m80 uses a two-phase cleanup path for preallocated hotplug slots. The host first
sends `DriveDetachRequest` to guestd and waits for a bounded
`DriveDetachResponse`. Only after guestd reports `Detached` or `NotMounted` for
the requested drive does the host retarget the Firecracker slot back to its
placeholder backing file with `PATCH /drives/{id}`.

Guestd handles detach per device. If the requested mount path is not mounted,
it returns `DriveDetachStatusKind::NotMounted` as an idempotent success. If the
path is mounted, guestd calls guest filesystem sync, unmounts the mount path
with `MNT_DETACH`, and returns `Detached`. I/O and unmount failures return a
per-device `Failed` status with a `DriveHotplugError`.

Host detach failures consume the running sandbox. A failed guest ACK, malformed
or missing response, failed placeholder PATCH, or timeout leaves the slot in an
unknown state, so m80 force-kills/deletes the VM and returns the original typed
error.

Evidence:

- `crates/m80-guestd/src/connection/hotplug.rs` dispatches
  `DriveDetachRequest`, syncs, unmounts, and returns per-device status.
- `crates/m80-firecracker/src/lifecycle/hotplug.rs` sends the detach request,
  validates the ACK, and patches the slot back to its placeholder file.
- Unit tests in both modules cover successful detach, idempotent not-mounted,
  failed status propagation, and request validation.
