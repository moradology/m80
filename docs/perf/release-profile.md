# Release Profile Delta

Bead: `m80-jp6ik.18`.

Raw artifact:

- `crates/m80-firecracker/benches/snapshots/release-profile-delta.json`

## Method

Run date: 2026-05-13.

Baseline build:

```sh
cargo build --release -p m80-cli
stat -c '%s %n' target/release/m80
N=50 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped \
  IMAGE_BUILD_DIR_MINIMAL=/tmp/m80-build/minimal-jp6ik42 \
  M80_BIN=./target/release/m80 ./scripts/bench-cold-launch.sh
```

Then the root `Cargo.toml` release profile was changed to `lto = "thin"`,
`codegen-units = 1`, `strip = "symbols"`, and `panic = "abort"`, and the same
build and bench commands were run again.

`cargo test -p m80-cli --no-run` succeeded. `cargo rustc -p m80-cli --test
parse_args -- --print cfg` reported `panic="unwind"`, so test builds keep
unwinding diagnostics even though release builds now abort on panic.

## Results

| profile | `target/release/m80` size | wallclock P50 | P95 | P99 | `phase_1_run_root_prep` P50 | `phase_12b_ready_accept` P50 |
|---|---:|---:|---:|---:|---:|---:|
| default release | 6,780,216 bytes | 1689 ms | 1694 ms | 1696 ms | 73 us | 1,023,051 us |
| tuned release | 4,415,472 bytes | 1595 ms | 1695 ms | 1695 ms | 72 us | 1,024,638 us |

The binary size dropped by 2,364,744 bytes, or 34.88%. The literal <=4,000,000
byte target did not land; the tuned binary is 4,415,472 bytes.

## Interpretation

The release profile is useful for binary size, but the N=50 cold-launch result
does not justify claiming a launch-path win from binary load. The direct proxy
phase, `phase_1_run_root_prep`, moved by -1 us P50 and +1 us P95. Overall
wallclock P50 improved by 94 ms, but `phase_12b_ready_accept` regressed by only
1.587 ms P50 and the large `phase_9_jailer_launch` movement is not attributable
to the Cargo profile from this single before/after pair.

The safe conclusion is: keep the profile for the 34.88% binary reduction and no
measured launch-path regression, but do not describe it as satisfying the <=4 MB
target or as a proven runtime optimization.
