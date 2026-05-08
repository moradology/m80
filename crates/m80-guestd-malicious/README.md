# `m80-guestd-malicious`

Test-only adversarial guest daemon used by real-KVM wire-protocol tests.

## Black-box contract

`m80-guestd-malicious` is not a production daemon and is not a dependency of
`m80-guestd`. Image-build tests may install this binary at `/m80-guestd` in a
dedicated malicious test image. The binary speaks only enough of the guestd
startup contract to let the host boot a VM, receive the inverted readiness
signal, and connect to the guest exec vsock port.

Attack mode selection is explicit:

- `--attack <name>` for direct host-side checks.
- `M80_MALICIOUS_ATTACK=<name>` for host process smoke checks.
- `m80.malicious_attack=<name>` on the guest kernel command line when running
  as PID 1 inside a test image.

Current modes:

- `noop` binds the standard guest vsock listener, sends the standard readiness
  byte to the host, and accepts connections without emitting adversarial frames.
- `oversized_length` binds the standard guest vsock listener, sends the
  readiness byte, and writes only a four-byte length prefix larger than
  `m80_proto::MAX_FRAME_BYTES` on each accepted connection.
- `truncated_frame` binds the standard guest vsock listener, sends the
  readiness byte, writes an in-cap frame length, writes fewer body bytes than
  promised, and closes the connection.
- `unknown_variant` binds the standard guest vsock listener, sends the
  readiness byte, writes a valid envelope with out-of-schema payload field
  number 255, and closes the connection.
- `response_type_mismatch` binds the standard guest vsock listener, sends the
  readiness byte, reads the host request, echoes its request id, writes an
  `exec_exit` envelope containing a `file_read_response` payload, and closes
  the connection.

## Non-goals

- No production fallback path.
- No compatibility with normal `m80-guestd` request handling.
- No tolerant parsing of unknown attack names. Unknown modes fail at startup.
