# Tokio runtime construction cost

Bead: `m80-jp6ik.48`

This is the E8 measurement from `docs/perf/measurement-playbook.md`. It checks
whether the two per-launch `tokio::runtime::Builder::new_current_thread()` call
sites are expensive enough to justify the sync-netlink replacement candidate
(`m80-jp6ik.31`).

## Run

Command:

```sh
PHASE_JSONL=crates/m80-firecracker/benches/tokio-runtime-cost.jsonl \
  EGRESS=outbound N=20 KIND=minimal SKIP_LOADED=1 ./scripts/bench-cold-launch.sh
```

`WARMUP=2` was left at the default. Attempts 1 and 2 were discarded from the
wallclock, phase, and JSONL artifacts. The committed JSONL contains attempts 3
through 22.

Artifacts:

- `crates/m80-firecracker/benches/tokio-runtime-cost.jsonl`
- `crates/m80-firecracker/benches/snapshots/tokio-runtime-cost-N20.json`
- JSONL sha256: `283096bbf4ae994e9f81ceabfd65cdc9723d9c3a08408fb3296eaad2594e91c8`
- snapshot sha256: `40c83cea02f12979483fa048e8c2b6e46dbb88a95262fed99cc490f097d7e3f7`

Host and substrate:

- host: `vulcan`
- kernel: `Linux 6.17.0-22-generic #22-Ubuntu SMP PREEMPT_DYNAMIC Fri Mar 13 12:04:44 UTC 2026 x86_64`
- CPUs: 48
- Firecracker/Jailer: `v1.15.1`
- image kind: minimal stock kernel, rebuilt with the measurement guestd
- image path: `/tmp/m80-build/minimal`
- image hashes:
  - `vmlinux`: `c453f36520d2f2792ab8e4532a814e4a647a4a41a4c94d4e9083a502800159b1`
  - `output.ext4`: `1d13e9b0652aadf6c4a7c91476dc467dd56cd023076f2d58559dd42a540f3ccd`
  - `output.ext4.manifest.json`: `a44d4f2a268f03e8b7c010a59554bba07643c893d529e5b3eb7277614d807a2b`
- `target/release/m80`: `92c879cbdf5170a5724fe8b0cdc81683d272a7205b9744565b3b465817e1be38`
- `target/x86_64-unknown-linux-musl/release/m80-guestd`: `d4256c6b7a4c0ba6a6954f6e55490c2d530359948e521efc48a67824fef64ef0`

## Launch Shape

| cell | successes | failures | wallclock P50 | wallclock P95 | `phase_12b_ready_accept` P50 | P95 |
|---|---:|---:|---:|---:|---:|---:|
| minimal outbound idle | 20 | 0 | 2138 ms | 2140 ms | 1020.409 ms | 1033.735 ms |

The outbound-specific setup phases are much larger than runtime construction:

| phase | P50 | P95 |
|---|---:|---:|
| `phase_6_network_realize` | 55.059 ms | 59.307 ms |
| `phase_7_outbound_guest_config` | 37.487 ms | 42.458 ms |

## Runtime Construction

| call site | phase marker | min | P50 | P75 | P90 | P95 | max |
|---|---|---:|---:|---:|---:|---:|---:|
| host `m80-net-outbound` | `phase_6_net_outbound_tokio_runtime_build` | 15 us | 20 us | 21 us | 22 us | 26 us | 34 us |
| guestd PID-1 network | `phase_12b_guest_tokio_runtime_build_guestd_network` | 24 us | 26 us | 27 us | 27 us | 30 us | 37 us |

## Interpretation

Both measured runtime-construction call sites are below 0.04 ms at max and far
below the playbook's 3 ms reconsideration threshold. The candidate's expected
5-10 ms per-call-site saving is not present on this host.

Close `m80-jp6ik.31` as not justified for the perf epic. The remaining outbound
network cost is real, but it is in link creation, TAP/bridge setup, iptables,
and guest network configuration, not Tokio runtime construction.
