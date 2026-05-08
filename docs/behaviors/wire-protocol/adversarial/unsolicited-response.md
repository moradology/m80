# Unsolicited Response

## Behavior

A malicious guest peer can accept a host connection and immediately write a
response frame without reading the request the host just sent on that channel.
m80 does not maintain a pre-request receive loop for guest-originated frames;
the unsolicited frame is observed by the active request reader and rejected by
the normal request-correlation check:

```text
FcError::Protocol(WireProtocolError::RequestIdMismatch {
    context: "exec exit",
    expected: "<active exec request id>",
    got: Some("unsolicited-response"),
})
```

The current policy is reject-and-teardown for the affected channel. The host
does not treat an unsolicited response as output for the active request, does
not silently drop it and wait for another terminal frame, and does not leak the
fabricated response into another request. Diagnostics record the mismatch and
the fabricated `unsolicited-response` id, and sandbox cleanup still succeeds.

## Evidence

- `crates/m80-guestd-malicious/src/main.rs` mode `unsolicited_response`
  writes a valid `exec_exit` response for `unsolicited-response` immediately
  after accepting the channel, without reading the host request.
- `crates/m80-firecracker/tests/malicious/unsolicited_response.rs::unsolicited_response_is_rejected_as_request_id_mismatch_and_diagnosed`
  launches the malicious guestd image, sends a normal exec request, asserts the
  documented request-id mismatch policy, checks diagnostics, and tears down the
  sandbox.

The real-KVM test is ignored by default because it needs
`M80_MALICIOUS_ARTIFACT_DIR` to point at image artifacts built with
`m80-guestd-malicious` installed as `/m80-guestd`.
