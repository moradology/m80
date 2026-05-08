# Unknown Payload Variant

## Behavior

A malicious guest peer can send a structurally valid envelope with a protobuf
payload field number that is outside the current `WireEnvelope.payload` oneof.
The host fails closed before typed payload conversion and surfaces:

```text
FcError::Protocol(WireProtocolError::MalformedPeer(
    "unknown envelope field: 255",
))
```

This is intentionally stricter than prost's default unknown-field behavior.
`m80-proto` rejects top-level envelope fields outside the current schema so an
unknown payload tag cannot be silently dropped and later reported as merely a
missing payload.

## Evidence

- `crates/m80-proto/tests/envelope_unknown_fields.rs::unknown_top_level_envelope_field_fails_closed_with_field_number`
  pins the raw envelope decoder behavior.
- `crates/m80-guestd-malicious/src/main.rs` mode `unknown_variant` emits a
  valid envelope body with field `255` as an empty length-delimited payload.
- `crates/m80-firecracker/tests/malicious/unknown_variant.rs::unknown_variant_tag_returns_malformed_peer_with_field_number`
  launches the malicious guestd image, sends a normal exec request, asserts the
  typed malformed-peer error includes field `255`, checks diagnostics, and
  tears down the sandbox.

The real-KVM test is ignored by default because it needs
`M80_MALICIOUS_ARTIFACT_DIR` to point at image artifacts built with
`m80-guestd-malicious` installed as `/m80-guestd`.
