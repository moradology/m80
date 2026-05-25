# Vsock Channel Robustness

`m80-vsock` treats the Firecracker UDS bridge handshake and protobuf frame
stream as fail-closed boundaries.

Handshake acknowledgement lines are capped at 64 bytes and must end in `\n`.
Oversized, empty, unterminated, non-UTF-8, or non-`OK ` replies return
`VsockError::HandshakeFailed` before application frames can be exchanged.

Typed receives reject an envelope whose `kind` string does not match the
requested payload type. A write-only `ChannelSender` may send same-connection
control frames while the owning `Channel` waits for a response. Dropping a
`Channel` closes the stream and signals EOF to the peer without unlinking the
host-side UDS.

Tests:

- `crates/m80-vsock/tests/handshake.rs::ok_handshake_without_newline_is_rejected`
- `crates/m80-vsock/tests/handshake.rs::oversized_ok_handshake_line_is_rejected`
- `crates/m80-vsock/tests/frame_round_trip.rs::unknown_envelope_kind_string_is_rejected_on_typed_recv`
- `crates/m80-vsock/tests/frame_round_trip.rs::cloned_sender_can_send_while_channel_waits_to_recv`
- `crates/m80-vsock/tests/drop_cleanup.rs::dropping_channel_signals_eof_to_peer`
