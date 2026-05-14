# Jailer Micro-Cuts

Date: 2026-05-14.

Bead: `m80-jp6ik.22`.

## Change

The jailer micro-cut batch landed as four small commits:

- `d853199` writes compact `jailer-plan.json` / `jailer-state.json` instead of
  pretty JSON on the hot path.
- `eb5cdd5` omits `MS_REC` for flat file-to-file bind mounts while keeping
  recursive bind mounts for directories.
- `8e205c0` sets `umask(0)` during jail materialization and relies on requested
  mkdir modes instead of a follow-up chmod.
- `9f44d0c` replaces the inherited-FD `/proc/self/fd` scan with
  `close_range(3, UINT_MAX, 0)` through `m80-close-range`.

## Functional Evidence

Local coverage already pinned each behavior:

- `cargo test -p m80-jailer bind_mount_flags --lib`
- `cargo test -p m80-jailer --test jailer asset_binding`
- `cargo test -p m80-jailer`
- `cargo test -p m80-jailer-harden`
- `cargo test --manifest-path support/m80-close-range/Cargo.toml`

Privileged smoke:

```sh
sudo env PATH="$PATH" cargo test -p m80-jailer-harden --test integration_root \
  wrapper_applies_inherited_hardening_before_exec -- --ignored --exact --nocapture
```

Result: passed, 1/1. This covers the kernel-touching close-range hardening path
and verifies inherited file descriptors above stdio are closed before the final
exec.

## Phase-9 Measurement

Raw artifacts:

- `crates/m80-firecracker/benches/snapshots/jailer-microcuts-before-N50.json`
- `crates/m80-firecracker/benches/snapshots/jailer-microcuts-after-N50.json`

Before tree: `d853199^` (`1ff6409`) with schema-4 minimal image
`/tmp/m80-build/minimal-jp6ik42`.

After tree: current `HEAD` with schema-5 minimal image `/tmp/m80-build/minimal`.

The schema differs because the old tree cannot read schema 5 and the current
tree cannot read schema 4. The measured phase is `phase_9_jailer_launch`, so
the comparison is still useful for the host-side jailer launch path, but the
run is not a perfectly identical whole-launch cell.

Commands:

```sh
N=50 WARMUP=2 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped \
  IMAGE_BUILD_DIR_MINIMAL=/tmp/m80-build/minimal-jp6ik42 \
  M80_BIN=./target/release/m80 \
  M80_JAILER_HARDEN_BIN=/tank/projects/m80/target/release/m80-jailer-harden \
  bash scripts/bench-cold-launch.sh

N=50 WARMUP=2 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped \
  IMAGE_BUILD_DIR_MINIMAL=/tmp/m80-build/minimal \
  M80_BIN=./target/release/m80 \
  M80_JAILER_HARDEN_BIN=/tank/projects/m80/target/release/m80-jailer-harden \
  bash scripts/bench-cold-launch.sh
```

Results:

| Tree | phase_9 P50 | phase_9 P95 | phase_9 max |
|---|---:|---:|---:|
| before | 15.678 ms | 31.686 ms | 31.825 ms |
| after | 15.583 ms | 31.724 ms | 31.921 ms |

Interpretation: the micro-cuts are functionally correct, but this host does not
show the expected 1.5-2.5 ms combined `phase_9_jailer_launch` win. The measured
P50 delta is about 0.095 ms and the tail is unchanged. Keep the implementation
for syscall and allocation cleanup, not as a material cold-launch optimization.
