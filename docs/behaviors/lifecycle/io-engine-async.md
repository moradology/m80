# Behavior: Async I/O Engine For Writable Preboot Drives

**Bead:** `m80-jp6ik.9`
**Date:** 2026-05-13
**Tests:** `crates/m80-firecracker-client/tests/put_each_resource.rs`,
`crates/m80-firecracker/src/preboot_tests.rs`

## Contract

`m80-firecracker-client::DriveConfig` carries an optional typed
`IoEngine::{Sync, Async}` field that serializes to Firecracker's PascalCase
wire values. When unset, `io_engine` is omitted so Firecracker keeps its
default behavior.

Cold-boot preboot planning sets `io_engine = Async` on the writable
`rootfs_overlay` and optional `workspace` drives. The shared read-only
`rootfs` drive and preallocated hotplug placeholder drives omit `io_engine`.

The jailer hardening defaults leave `RLIMIT_MEMLOCK` inherited. Firecracker's
async block engine uses io_uring, and a forced `memlock=0` makes launch fail
before `InstanceStart`.

This is a hard cutover for m80's own writable preboot drives: callers do not
choose the overlay/workspace engine through a compatibility knob.
