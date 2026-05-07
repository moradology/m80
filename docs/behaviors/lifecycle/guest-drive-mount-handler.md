# Guest Drive Mount Handler

Behavior captured by bead `m80-iswt.5`.

## Present-tense statement

`m80-guestd` handles `DriveMountRequest` on the same length-prefixed protobuf
connection as exec, PTY, file ops, metrics, and shutdown. The handler mounts
requested preallocated Firecracker drive slots inside the guest, reports one
typed status per requested device, and optionally returns opaque tenant-identity
bytes read from the mounted filesystem.

## Contract

- Requests use
  `DriveMountSpec { drive_id, device_path, mount_path, identity_path }`.
- `drive_id` is a Firecracker preallocated-slot id in the form
  `hotplug_slot_N`. `device_path` is the exact guest block-device path to
  mount, such as `/dev/vdd`. The host supplies this path because it owns the
  actual drive PUT layout for the VM.
- If the expected device node already exists, guestd proceeds immediately.
  Otherwise it starts the uevent listener and waits for a matching block
  `DEVNAME` with a bounded timeout; it does not busy-poll.
- If `mount_path` is already mounted from the expected device, the request is
  idempotent and returns `AlreadyMounted` without calling `mount(2)` again.
- If `mount_path` is already mounted from a different source, the request
  fails closed with `MountFailed`.
- Multi-device requests are partial-success: every requested device gets a
  `DriveMountStatus`; a failed device does not roll back a device mounted
  earlier in the same request.
- `identity_path`, when present, is read exactly as a guest path. The bytes are
  returned as `TenantIdentityReport`; m80 treats the contents as opaque.
- Missing identity files return `IdentityMissing`; unreadable identity files
  return `IdentityReadFailed`.

## Tests

- `crates/m80-guestd/src/connection/hotplug.rs` tests cover requested-device
  wait behavior, invalid device paths failing closed, mounted success with
  identity bytes, partial-success reporting, idempotent retry without a second
  mount call, handler request/response framing, and mountinfo source parsing.
