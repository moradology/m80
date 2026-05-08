# Response Type Mismatch

## Behavior

A malicious guest peer can receive a normal typed request and reply with a
structurally valid envelope whose `kind` claims to be the expected terminal
response while the protobuf `oneof` carries a different response payload. The
host rejects the frame during typed payload conversion and surfaces:

```text
FcError::Protocol(WireProtocolError::MalformedPeer(
    "unexpected protobuf payload for exec_exit: file_read_response",
))
```

The malicious peer echoes the active request id, so this behavior is pinned as
a response type mismatch rather than a stale-request or missing-request-id
failure. The affected request fails, diagnostics record the expected and
observed payload kinds, and sandbox cleanup still succeeds.

## Evidence

- `crates/m80-guestd-malicious/src/main.rs` mode
  `response_type_mismatch` reads the host request, echoes its `request_id`,
  writes an envelope with `kind = "exec_exit"`, and places a
  `file_read_response` payload inside it.
- `crates/m80-firecracker/tests/malicious/response_type_mismatch.rs::response_type_mismatch_returns_malformed_peer_with_expected_and_observed_kinds`
  launches the malicious guestd image, sends a normal exec request, asserts the
  typed malformed-peer error names both `exec_exit` and `file_read_response`,
  checks diagnostics, and tears down the sandbox.

The real-KVM test is ignored by default because it needs
`M80_MALICIOUS_ARTIFACT_DIR` to point at image artifacts built with
`m80-guestd-malicious` installed as `/m80-guestd`.
