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

`noop` is the initial harness smoke mode. It binds the normal guest exec vsock
port, sends the normal one-byte readiness signal to the host, and accepts
connections without producing adversarial frames. It proves the artifact can
replace production guestd inside the image and reach the host readiness path.

Later L12 leaves add frame-producing modes such as oversized length, truncated
frame, unknown variant, wrong response type, bogus request id, unsolicited
response, unsolicited flood, and slowloris.

## Evidence

- `crates/m80-guestd-malicious/src/main.rs` implements mode selection and the
  `noop` readiness peer.
- `crates/m80-guestd-malicious/tests/cli.rs` verifies the built artifact's
  host-side selector behavior.
- `crates/m80-firecracker/tests/malicious_harness_smoke.rs` is the ignored
  real-KVM smoke for an image built with the malicious artifact.
