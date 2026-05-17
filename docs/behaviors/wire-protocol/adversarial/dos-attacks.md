# DoS Attacks

## Behavior

The L12 DoS coverage pins two guest-to-host policies:

- `unsolicited_flood`: a malicious guest accepts an exec channel and writes a
  bounded burst of unsolicited terminal responses without reading the host
  request. The host rejects the first fabricated response as a request-id
  mismatch, records diagnostics, and does not keep reading attacker-controlled
  frames into unbounded memory.
- `slowloris`: a malicious guest writes a valid in-cap frame prefix plus a
  short partial body, then keeps the channel open without making progress. The
  caller's host exec deadline bounds the wait and surfaces:

```text
FcError::Protocol(WireProtocolError::ReadTimeout {
    context: "streaming exec",
})
```

Both policies fail the affected request and leave teardown to the normal
sandbox cleanup path. m80 does not silently continue on the same poisoned
channel after either DoS shape.

## Evidence

- `crates/m80-guestd-malicious/src/main.rs` mode `unsolicited_flood` writes
  512 fabricated `exec_exit` responses; mode `slowloris` writes a 16-byte
  declared length, sends only a short partial body, flushes, and then stalls.
- `crates/m80-firecracker/src/lifecycle/exec.rs` uses
  `Channel::recv_raw_with_deadline` for host-deadline execs; deadline expiry
  before the terminal frame maps to `WireProtocolError::ReadTimeout` instead
  of a raw transport error.
- `crates/m80-firecracker/tests/malicious/dos_attacks.rs::unsolicited_flood_fails_fast_without_host_memory_growth`
  asserts the flood fails on the first fabricated request id, bounds host RSS
  growth, checks diagnostics, and tears down the sandbox.
- `crates/m80-firecracker/tests/malicious/dos_attacks.rs::slowloris_partial_frame_times_out_without_stuck_reader`
  asserts the slowloris reader unblocks within the test deadline with
  `ReadTimeout`, checks diagnostics, and tears down the sandbox.

The real-KVM tests are ignored by default because they need
`M80_MALICIOUS_ARTIFACT_DIR` to point at image artifacts built with
`m80-guestd-malicious` installed as `/m80-guestd`.
