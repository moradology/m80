# CPU Template Delta

**Bead:** `m80-jp6ik.27`
**Protocol:** `docs/perf/measurement-playbook.md#e12-cpu-template-delta`
**Artifact:** `crates/m80-firecracker/benches/snapshots/cpu-template-delta.json`
**Date:** 2026-05-13

## Host

- kernel: `Linux vulcan 6.17.0-22-generic #22-Ubuntu SMP PREEMPT_DYNAMIC Fri Mar 13 12:04:44 UTC 2026 x86_64 GNU/Linux`
- Firecracker: `v1.15.1`
- CPU frequency driver/governor: `acpi-cpufreq` / `schedutil`
- image: `minimal`, `stock` kernel
- artifacts: `/tmp/m80-build/minimal/vmlinux`, `/tmp/m80-build/minimal/output.ext4`
- run root: `/var/lib/m80-run`
- command: `N=50 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh`

## Result

The code change landed the hard cutover: default `SandboxConfig` now omits
`cpu_template` from the Firecracker machine-config PUT, and callers can opt in
with `SandboxConfig::cpu_template = Some(CpuTemplate::T2 | CpuTemplate::C3)`.

The measured phase_12a target did not land on this host/session.

| cell | baseline T2 P50 | default-none P50 | delta |
|---|---:|---:|---:|
| `phase_12a_instance_start` | 18,393 us | 18,591 us | +198 us |
| wallclock | 1,728 ms | 1,729 ms | +1 ms |

The target was a >=50 us P50 drop. This run measured a small regression/noise
band instead, so the bead should not be closed as a verified latency win from
this evidence. The behavior remains useful as a semantics cleanup: m80 no
longer pays for silent AWS live-migration masking on the default same-host
snapshot path, and cross-host restore can be added later as explicit CPU-feature
parity admission.

## Snapshot Restore

Same-host snapshot/restore still works without the default T2 mask:

```sh
sudo env PATH="$PATH" \
  IMAGE_BUILD_DIR=/tmp/m80-build/minimal \
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
  cargo test -p m80-firecracker --test snapshot_integration \
    capture_then_restore_round_trip -- --ignored --nocapture
```

Result: 1 passed.

## Tradeoff

The default snapshot contract is same-host restore. Future cross-host restore
must verify host CPU-feature parity before accepting a restore target; m80
should not silently mask the CPU surface by default as a substitute for that
admission check.
