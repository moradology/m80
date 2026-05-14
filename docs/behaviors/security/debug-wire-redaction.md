# Debug Wire Redaction

Bead: `m80-8emae.11`

`M80_DEBUG_WIRE=vsock` is an operator diagnostic for the host-to-guest vsock
channel. It must not turn `ExecRequest.env` into a trace-log secret sink.

Outbound `exec_request` previews are formatted from a cloned `RawEnvelope`.
Before the preview bytes are encoded, the clone's `env` entries are cleared and
the preview string appends `env=[N entries redacted]`. The original envelope is
still sent unchanged to the guest.

Other outbound vsock frame kinds are previewed as their encoded bytes. Inbound
vsock frames log their payload kind, not a byte preview. The Firecracker REST
debug target, `M80_DEBUG_WIRE=fcrest`, remains a raw Firecracker API dump and
must only be enabled when those request and response bodies are safe to log.

Tests:

- `crates/m80-vsock/src/debug_wire.rs::tests::exec_request_preview_redacts_env_values`
- `crates/m80-vsock/tests/debug_wire_redaction.rs::debug_wire_redacts_exec_request_env_values`
