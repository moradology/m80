# Jailer Asset Binding

## kernel-bin-ro

m80 exposes immutable launch artifacts to the jail as read-only inputs. In the
current Firecracker launch path, the kernel is bound read-only at `kernel`, and
the shared base rootfs is bound read-only at `rootfs.ext4`. The Firecracker
binary itself is supplied through `JailerConfig::firecracker_bin`; the official
jailer copies that executable into the jail root with `O_NOFOLLOW`, rejects
hard-linked destinations, chowns the copy to the jail uid/gid, and uses the
copied in-jail binary for the final exec. Under m80's hardening wrapper umask,
the live copy is owner-only (`0700`). m80 does not bind-mount the Firecracker
executable.

This is a hard cutover from predecessor's older in-jail names
`bin/firecracker` and `kernel/vmlinux`.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`JAILER_BIN_PATH` line 30, `JAILER_KERNEL_PATH` line 31, and
`build_jailer_plan` lines 288-296.

Test: `crates/m80-jailer/tests/jailer/asset_binding.rs::binds_kernel_and_rootfs_ro`.
Test: `crates/m80-firecracker/tests/end_to_end_real_kvm.rs::end_to_end_real_kvm_jailer_security_parity`.

## drives-rw

m80 exposes writable per-VM drive images as read-write bind mounts. The rootfs
overlay is bound at `rootfs.overlay.ext4`; when a workspace is configured, its
scratch image is bound at `scratch.ext4`. These are writable because
Firecracker must open them as mutable block-device backing files.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`JAILER_ROOTFS_PATH` line 32, `JAILER_WORKSPACE_PATH` line 33, and
`build_jailer_plan` lines 297-304.

Test: `crates/m80-jailer/tests/jailer/asset_binding.rs::binds_drives_rw`.

## sockets-inside

The Firecracker API socket and vsock muxer socket are declared as socket paths
inside the jail. m80 uses `firecracker.sock` and `vsock.sock` under the jail
root. The jailer plan records these as `PlanStep::Socket`; the files are not
host artifacts copied into the jail.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`JAILER_API_SOCKET_PATH` line 34, `JAILER_VSOCK_SOCKET_PATH` line 35, and
`build_jailer_plan` lines 305-310.

Test: `crates/m80-jailer/tests/jailer/asset_binding.rs::sockets_created_inside_jail`.

## host-only-artifacts

Ownership markers, persisted jailer plan/state, boot identity, console log,
diagnostics log, and metrics snapshots remain host-side run-dir artifacts. They
are not bind-mounted into the jail and do not become `PlanStep` entries.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`build_jailer_plan` lines 311-334.

Test: `crates/m80-jailer/tests/jailer/asset_binding.rs::host_only_artifacts_listed`.

## replayable

`Plan::compute` is pure and deterministic. Given the same `JailerConfig`, it
emits the same ordered `PlanStep` list and serializes to the same JSON. Recovery
uses the persisted `jailer-plan.json` rather than recomputing an approximate
teardown plan.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`build_jailer_plan` lines 264-339 and `read_prepared_jailer_plan` lines
360-373.

Test: `crates/m80-jailer/tests/plan_serde.rs::plan_round_trips_byte_equal_via_compact_json`.
Test: `crates/m80-jailer/tests/plan_compute.rs::determinism_byte_equal_json`.
