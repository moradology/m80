# Oversized Length Prefix

## Behavior

A malicious guest peer can announce a protobuf frame length larger than
`m80_proto::MAX_FRAME_BYTES` and then send no body. The host must reject that
frame from the four-byte prefix alone. It must not allocate the announced body,
panic, or continue reading from the poisoned channel.

On the `RunningSandbox::exec` path, the oversized prefix maps to:

```text
FcError::Protocol(WireProtocolError::OversizedFrame { size, limit })
```

The affected request fails, diagnostics record the protocol error, and the
sandbox can still be force-killed and deleted without leaking the run
directory.

## Evidence

- `crates/m80-guestd-malicious/src/main.rs` mode `oversized_length` writes only
  the four-byte prefix `MAX_FRAME_BYTES + 1`.
- `crates/m80-firecracker/tests/malicious/oversized_length.rs::oversized_length_prefix_returns_typed_protocol_error`
  launches the malicious guestd image, sends a normal exec request, asserts the
  typed oversized-frame error, bounds RSS growth, checks diagnostics, and
  tears down the sandbox.

The real-KVM test is ignored by default because it needs
`M80_MALICIOUS_ARTIFACT_DIR` to point at image artifacts built with
`m80-guestd-malicious` installed as `/m80-guestd`.
