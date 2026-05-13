# Frame Write Coalescing

`m80-proto::write_raw_frame` serializes a frame body, prefixes it with the
four-byte big-endian length, and sends the complete prefix+body buffer through a
single `write_all` call.

`m80-vsock::Channel::send` and `ChannelSender::send` construct the raw envelope
from a borrowed typed envelope. The caller keeps ownership of the typed envelope,
while the wire layer owns the protobuf payload it is about to encode.

The wire format is unchanged: readers still consume the same four-byte length
prefix followed by the protobuf body.

Tests:

- `crates/m80-proto/src/framing.rs::write_raw_frame_coalesces_prefix_and_body`
- `crates/m80-vsock/tests/frame_round_trip.rs::send_recv_envelope_round_trips`
