# Async Drive I/O Engine

Bead: `m80-jp6ik.9`.

## Implemented Surface

`DriveConfig::io_engine` now carries Firecracker's optional block I/O engine
field. Cold preboot sets `Async` for the writable `rootfs_overlay` and optional
`workspace` drives, while the shared read-only `rootfs` and preallocated hotplug
placeholder drives leave the field omitted.

## Smoke Result

Command:

```sh
sudo env PATH="$PATH" IMAGE_BUILD_DIR=/tmp/m80-build/minimal \
  M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
  M80_JAILER_BIN=/opt/firecracker/bin/jailer \
  M80_JAILER_HARDEN_BIN="$PWD/target/release/m80-jailer-harden" \
  M80_KERNEL_IMAGE=/tmp/m80-build/minimal/vmlinux \
  M80_KERNEL_KIND=stock \
  M80_ROOTFS_IMAGE=/tmp/m80-build/minimal/output.ext4 \
  M80_RUN_ROOT=/var/lib/m80-run \
  M80_FIRECRACKER_VERSION=v1.15.1 \
  M80_JAIL_UID="$(id -u)" \
  M80_JAIL_GID="$(getent group kvm | cut -d: -f3 || id -g)" \
  cargo test -p m80-firecracker --test end_to_end_real_kvm \
    end_to_end_real_kvm_boot_exec_stop_delete -- --ignored --nocapture
```

Result: passed, 1/1.

An earlier run with `ResourceLimits::default().memlock = Some(0)` failed during
the drive PUT with Firecracker's `io_uring_setup: Out of memory (os error 12)`.
The cutover therefore leaves `RLIMIT_MEMLOCK` inherited by default.

## Close Gate

The functional cutover is verified. The exec-with-I/O gate was measured on
2026-05-14 with a temporary legacy-compatible version of
`drive_sync_latency.rs`, because the historical before/after commits predate
the later `drive_cache_type` field.

Raw artifacts:

- `crates/m80-firecracker/benches/snapshots/io-engine-sync-N10.json`
- `crates/m80-firecracker/benches/snapshots/io-engine-async-N10.json`

Comparison:

- before tree: `5f2f281^` (`aef48f8`), Firecracker default synchronous drive
  engine;
- after tree: `5f2f281`, writable drives set `io_engine = Async`;
- image: schema-4 minimal bundle `/tmp/m80-build/minimal-jp6ik42`;
- workload: one real-KVM VM, `N=10`, each exec sample performs 100
  `dd bs=4096 count=1 conv=fsync` writes followed by `sync`.

| tree | launch | sync-heavy exec P50 | P95 | max | mean |
|---|---:|---:|---:|---:|---:|
| Sync/default | 924 ms | 60 ms | 79 ms | 79 ms | 67 ms |
| Async | 937 ms | 60 ms | 80 ms | 80 ms | 69 ms |

Result: no observed exec-with-I/O win on this host. The implementation remains
functionally correct and keeps the Firecracker field available, but the
expected 10-30 ms improvement did not reproduce in this workload. No density
variance win is claimed from this result.
