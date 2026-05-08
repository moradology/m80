# Bogus Request Id

## Behavior

A malicious guest peer can read a legitimate request and reply with a
structurally valid response whose `request_id` was never issued on the active
channel. The host checks the request id before accepting the terminal frame and
surfaces:

```text
FcError::Protocol(WireProtocolError::RequestIdMismatch {
    context: "exec exit",
    expected: "<active exec request id>",
    got: Some("malicious-stale-request-id"),
})
```

The current policy is fail-fast for the affected request. The host does not
try to reinterpret the bogus response as another caller's output and does not
wait indefinitely for a second terminal frame after the correlation check
fails. Diagnostics record the expected and observed ids, and sandbox cleanup
still succeeds.

## Evidence

- `crates/m80-guestd-malicious/src/main.rs` mode `bogus_request_id` reads the
  host request, then writes a valid `exec_exit` response for
  `malicious-stale-request-id`.
- `crates/m80-firecracker/tests/malicious/bogus_request_id.rs::bogus_request_id_returns_request_id_mismatch_with_expected_and_observed_ids`
  launches the malicious guestd image, sends a normal exec request, asserts the
  typed request-id mismatch includes both the active and fabricated ids, checks
  diagnostics, and tears down the sandbox.

The real-KVM test is ignored by default because it needs
`M80_MALICIOUS_ARTIFACT_DIR` to point at image artifacts built with
`m80-guestd-malicious` installed as `/m80-guestd`.
