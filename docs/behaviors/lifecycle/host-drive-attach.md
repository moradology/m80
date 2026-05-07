# Host Drive Attach

Captured by bead `m80-iswt.6`.

`m80-firecracker` exposes `RunningSandbox::attach_drive_verified` as the
host-side handoff for one preallocated tenant drive slot. The call consumes the
running sandbox, retargets the selected `hotplug_slot_N` with Firecracker
`PATCH /drives/{id}`, sends a protobuf `DriveMountRequest` over the guestd
vsock channel, waits for the guest's `DriveMountResponse`, and returns the
running sandbox only after the requested status is `Mounted` or
`AlreadyMounted` and the guest-reported opaque identity bytes exactly match the
caller expectation.

The call treats all failures after entry as contamination of the running VM.
Configuration errors, Firecracker PATCH failures, vsock/protocol failures,
guest mount failures, missing identity reports, and identity mismatches all
consume the sandbox and force-kill/delete the VM before returning the original
error. Guest mount errors are surfaced as `FcError::DriveHotplug`; identity byte
mismatches are surfaced as `FcError::TenantIdentityMismatch` with lengths only.
The expected and actual identity bytes are not rendered in the error.

Device paths are deterministic from Firecracker drive PUT order. With no
workspace drive, slot 0 is `/dev/vdc`; with a workspace drive, slot 0 is
`/dev/vdd`. Post-`z` suffixes use Linux virtio block naming such as
`/dev/vdaa`.

`HotplugDriveAttach::path_on_host` is the Firecracker-visible path for the new
backing file. Because m80 launches Firecracker inside a jail, this must be an
absolute path that is visible inside that jail namespace; the API rejects
relative paths before issuing a Firecracker PATCH.

Evidence:

- `crates/m80-firecracker/src/lifecycle/hotplug.rs` covers the host attach API,
  slot-to-device naming, response validation, and discard-on-failure path.
- Unit tests in `crates/m80-firecracker/src/lifecycle/hotplug.rs` pin device
  naming, successful identity verification, mismatch rejection, guest failure
  propagation, and relative path rejection.
