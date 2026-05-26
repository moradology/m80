# Malicious Guestd Harness

## Behavior

`m80-guestd-malicious` is a test-only guest daemon used by the L12 adversarial
wire suite. It is a separate crate from production `m80-guestd`, so release
builds of `m80-guestd` do not contain a dormant adversarial dispatch path.

To use it in a real-KVM test, build a dedicated image with
`m80-guestd-malicious` installed at `/m80-guestd`, then launch the VM with a
kernel command-line selector:

```text
m80.malicious_attack=noop
```

The selector can also come from direct host-side checks:

```text
m80-guestd-malicious --attack noop --check-config
M80_MALICIOUS_ATTACK=noop m80-guestd-malicious --check-config
```

Unknown attack names fail startup. That is intentional: adversarial tests must
name exactly the peer behavior they are exercising.

## Attack Modes

`noop` binds the normal guest exec vsock port, sends the normal one-byte
readiness signal to the host, and accepts connections without producing
adversarial frames. It proves the artifact can replace production guestd inside
the image and reach the host readiness path.

`no_ready` binds the normal guest exec vsock port and then parks forever without
sending the one-byte readiness signal. This mode is for launch-failure cleanup
tests that need a valid non-production guest artifact but must exercise
`GuestdReadyTimeout`.

`oversized_length` writes only a four-byte length prefix larger than
`m80_proto::MAX_FRAME_BYTES` on each accepted connection. This exercises the
host framing guard before the host allocates a frame body.

`truncated_frame` writes an in-cap length prefix, writes fewer body bytes than
declared, flushes, and closes the connection. This exercises host under-read
handling without relying on a malformed protobuf body.

`unknown_variant` writes a valid envelope with an out-of-schema protobuf payload
field number. This exercises the host's unknown envelope field rejection.

`response_type_mismatch` reads the host request, echoes its `request_id`, and
then writes an `exec_exit` envelope containing a `file_read_response` payload.
This exercises typed payload conversion after routing and request-id checks
have already accepted the frame.

`bogus_request_id` reads the host request, then writes a valid `exec_exit`
envelope for `malicious-stale-request-id`. This exercises host-side
request-correlation checks on a frame that is otherwise well formed.

`unsolicited_response` writes a valid `exec_exit` envelope for
`unsolicited-response` immediately after accepting the host channel, without
reading the host request. This records the current reject-and-teardown policy
for unsolicited guest output.

`unsolicited_flood` writes a bounded burst of fabricated `exec_exit` responses
without reading the host request. This exercises fail-fast handling and host
memory bounds for repeated unsolicited guest output.

`slowloris` writes an in-cap frame prefix plus a short partial body, flushes,
and then stops making progress while keeping the channel open. This exercises
host-deadline no-progress handling.

## Evidence

- `crates/m80-guestd-malicious/src/main.rs` implements mode selection and the
  `noop` readiness peer.
- `crates/m80-guestd-malicious/tests/cli.rs` verifies the built artifact's
  host-side selector behavior.
- `crates/m80-firecracker/tests/malicious_harness_smoke.rs` is the ignored
  real-KVM smoke for an image built with the malicious artifact.
