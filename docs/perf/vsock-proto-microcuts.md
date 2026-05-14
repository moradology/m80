# Vsock Proto Micro-Cuts

Date: 2026-05-14.

Bead: `m80-jp6ik.33`.

## Change

`m80-proto::write_raw_frame` now encodes the protobuf body, prefixes it with the
four-byte length, and sends the complete frame with one `write_all`. Before
`68d56fd`, it wrote the prefix and body with two separate `write_all` calls.

`RawEnvelope::from_typed` now borrows the typed envelope. The normal
`m80-vsock` send path no longer clones the typed envelope before converting it
to the raw protobuf envelope. Debug-wire tracing still clones the raw envelope
only when `M80_DEBUG_WIRE` enables the vsock trace.

## Harness

`crates/m80-proto/benches/vsock_proto_microcuts.rs` measures the hot paths over
a `UnixStream` pair with a draining reader thread:

- one typed `ExecRequest` frame, including typed-to-raw conversion and frame
  write;
- one streaming burst of 100 `ExecStdout` frames with 4096-byte chunks.

The comparison used the same harness copied into a temporary worktree at
`68d56fd^` (`8ac876d`) and the current tree after the micro-cut.

Commands:

```sh
N=100000 cargo bench -p m80-proto --bench vsock_proto_microcuts
N=50000 cargo bench -p m80-proto --bench vsock_proto_microcuts
```

## Results

| Tree | N | Single exec frame mean | 100 streaming frames mean |
|---|---:|---:|---:|
| `68d56fd^` | 100000 | 3003 ns | 458.243 us |
| current | 100000 | 1807 ns | 285.149 us |
| `68d56fd^` | 50000 | 3006 ns | 441.726 us |
| current | 50000 | 1783 ns | 291.358 us |

Observed deltas:

- single exec request frame: about 1.2 us faster;
- 100 streaming stdout frames: about 150-173 us faster.

The single-frame result is inside the bead's expected 1-3 us range. The
100-frame result is larger than the original 10-40 us estimate on this host,
because the second syscall was a material part of the per-frame UnixStream
write cost.

## Behavior Coverage

Wire-format behavior remains pinned by:

- `crates/m80-proto/src/framing.rs::write_raw_frame_coalesces_prefix_and_body`;
- `crates/m80-vsock/tests/frame_round_trip.rs::send_recv_envelope_round_trips`;
- `crates/m80-firecracker/tests/streaming_exec.rs::exec_and_exec_streaming_report_equivalent_output`
  for real-KVM streaming behavior.
