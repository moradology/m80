# Drive Cache Policy

Date: 2026-05-14.

Bead: `m80-jp6ik.38`.

## Change

Writable preboot drives (`rootfs_overlay` and optional `workspace`) default to
Firecracker `cache_type = Unsafe`. The shared read-only rootfs and preallocated
hotplug placeholders omit `cache_type`. Callers that need conservative host
sync semantics can set `SandboxConfig::drive_cache_type =
Some(CacheType::Writeback)`.

## Harness

`crates/m80-firecracker/benches/drive_sync_latency.rs` launches one real-KVM VM
with the selected writable-drive cache policy, warms it with one sync, then runs
`N` exec samples. Each sample performs 100 iterations of:

```sh
dd if=/dev/zero of="$d/file-$i" bs=4096 count=1 conv=fsync status=none
sync
```

The benchmark also records launch wall time for the single VM in each policy
cell. This is not a full cold-launch distribution, but it checks that the
cache-policy win is in the exec I/O phase rather than VM boot.

Commands:

```sh
cargo bench -p m80-firecracker --bench drive_sync_latency --features real-kvm-bench --no-run

sudo env PATH="$PATH" \
  M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
  M80_JAILER_BIN=/opt/firecracker/bin/jailer \
  M80_JAILER_HARDEN_BIN="$PWD/target/release/m80-jailer-harden" \
  M80_FIRECRACKER_VERSION=v1.15.1 \
  M80_KERNEL_IMAGE=/tmp/m80-build/minimal/vmlinux \
  M80_ROOTFS_IMAGE=/tmp/m80-build/minimal/output.ext4 \
  M80_RUN_ROOT=/var/lib/m80-run \
  M80_DRIVE_CACHE_TYPE=Writeback \
  N=10 \
  M80_DRIVE_SYNC_COUNT=100 \
  M80_DRIVE_SYNC_BENCH_OUTPUT="$PWD/crates/m80-firecracker/benches/snapshots/drive-sync-writeback-N10.json" \
  target/release/deps/drive_sync_latency-*

sudo env ... M80_DRIVE_CACHE_TYPE=Unsafe \
  M80_DRIVE_SYNC_BENCH_OUTPUT="$PWD/crates/m80-firecracker/benches/snapshots/drive-sync-unsafe-N10.json" \
  target/release/deps/drive_sync_latency-*
```

Raw artifacts:

- `crates/m80-firecracker/benches/snapshots/drive-sync-writeback-N10.json`
- `crates/m80-firecracker/benches/snapshots/drive-sync-unsafe-N10.json`

## Results

| cache_type | launch | sync-heavy exec P50 | P95 | max | mean |
|---|---:|---:|---:|---:|---:|
| Writeback | 1104 ms | 2082 ms | 2088 ms | 2090 ms | 2080 ms |
| Unsafe | 1055 ms | 72 ms | 82 ms | 83 ms | 75 ms |

`Unsafe` reduces the sync-heavy exec P50 by about 96.5% in this harness. The
single launch timings are within normal launch noise and do not show a
meaningful cold-launch effect, matching the expectation that cache policy
matters when the guest asks the writable block device to flush data.

## Smoke Coverage

The benchmark launches real Firecracker through the jailer with the typed
`cache_type` field applied to the writable preboot drive. Existing structural
tests continue to pin the JSON and preboot plan:

- `crates/m80-firecracker-client/tests/put_each_resource.rs::cache_type_uses_firecracker_pascal_case`
- `crates/m80-firecracker/src/preboot_tests.rs::writable_drive_cache_type_override_preserves_writeback`
