# Virtio-Rng Entropy Device

m80 adds Firecracker's default virtio-rng entropy device during cold preboot.
The preboot plan sends `PUT /entropy` with `{}` after any network-interface PUT
and before `/vsock`, then continues to `InstanceStart`.

The device is always on and has no `SandboxConfig` opt-out. Entropy
availability is VM mechanics, not adapter policy, and keeping it in the
baseline prevents first exec from depending only on slow guest interrupt timing
for `/dev/random` and `getrandom()`.

Errors from Firecracker map to
`m80_firecracker_client::ClientError::EntropyDeviceWriteFailed`; the
orchestrator then carries that client error with the surrounding preboot phase
context.

Tests:
- `crates/m80-firecracker-client/tests/put_each_resource.rs::put_entropy_device_sends_empty_config_json`
- `crates/m80-firecracker-client/tests/error_mapping.rs::entropy_device_400_returns_entropy_device_write_failed`
- `crates/m80-firecracker/src/preboot.rs::tests::entropy_device_put_before_vsock_and_start`
- `crates/m80-firecracker/src/preboot.rs::tests::preboot_put_phase_names_include_individual_devices`

Real-KVM evidence:
- N=10 smoke bench accepted `/entropy` and emitted `phase_11_put_entropy`.
- N=50 snapshot: `crates/m80-firecracker/benches/snapshots/2026-05-13T16:11:29+00:00.json`
- Command: `N=50 WARMUP=2 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped M80_BIN=./target/release/m80 ./scripts/bench-cold-launch.sh`
- `phase_11_put_entropy` P50 was 112 us; wallclock minimal/idle P50 was 1728 ms.
- One direct real-KVM exec read 256 bytes from `/dev/random` successfully with
  `time dd if=/dev/random of=/dev/null bs=256 count=1`; guest-reported
  `real` time was `0m 0.00s`.
