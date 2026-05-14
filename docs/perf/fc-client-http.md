# Firecracker Client HTTP Micro-Cuts

Date: 2026-05-14.

Bead: `m80-jp6ik.39`.

## Change

`m80-firecracker-client` now assembles each JSON HTTP request as one
header+body buffer and sends it with one `write_all`. Before `1ff6409`, the
helper wrote the header and body separately.

The VM-state and instance-action endpoints now serialize private typed payload
structs directly instead of building `serde_json::Value` through `json!`.

## Functional Evidence

Tests:

```sh
cargo test -p m80-firecracker-client send_json_writes_header_and_body_in_one_call --lib
cargo test -p m80-firecracker-client instance_action --test instance_action_serialization
```

Results: both passed.

The unit writer test pins the one-write request contract. The instance-action
test pins the typed payload shape used by Firecracker's `/actions` endpoint.

## Phase-11 Measurement

Raw artifacts:

- `crates/m80-firecracker/benches/snapshots/fc-http-before-N50.json`
- `crates/m80-firecracker/benches/snapshots/fc-http-after-N50.json`

Before tree: `1ff6409^` (`699f23e`).

After tree: `1ff6409`.

Both cells used the schema-4 minimal image bundle
`/tmp/m80-build/minimal-jp6ik42`.

Commands:

```sh
N=50 WARMUP=2 KIND=minimal SKIP_LOADED=1 KERNEL_KIND=stripped \
  IMAGE_BUILD_DIR_MINIMAL=/tmp/m80-build/minimal-jp6ik42 \
  M80_BIN=./target/release/m80 \
  M80_JAILER_HARDEN_BIN=/tank/projects/m80/target/release/m80-jailer-harden \
  bash scripts/bench-cold-launch.sh
```

Results:

| Tree | phase_11_rest_puts P50 | P95 | P99 | max |
|---|---:|---:|---:|---:|
| before | 1.326 ms | 1.541 ms | 1.687 ms | 1.752 ms |
| after | 1.309 ms | 1.533 ms | 1.610 ms | 1.882 ms |

Interpretation: the syscall contract is fixed and Firecracker still accepts the
requests. The full phase P50 improvement is small: about 17 us by the snapshot
summary, with a 77 us P99 improvement and a noisy max. This is at the low edge
of the original 20-50 us estimate, so treat the change as clean protocol-client
overhead removal rather than a large launch-path win.
