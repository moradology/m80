# Exec Host Deadline

`exec_with_max_duration` and `exec_streaming_with_max_duration` enforce the
caller budget on the host receive path, not only inside `m80-guestd`.

The host sends `max_duration_ms` on the envelope for guest-side process
management, then uses the same budget as a call-wide deadline while waiting for
response frames. If the deadline expires before a complete terminal frame
arrives, `m80-firecracker` returns `FcError::ExecTimeoutHost` and drops the
vsock channel.

`m80-vsock` implements this with `Channel::recv_raw_with_deadline`, which
checks the deadline before every underlying socket read. A compromised guestd
cannot keep a host exec call alive indefinitely by writing partial frame bytes
just under the deadline wrapper's per-read timeout. Normal channel reads have
no bridge I/O timeout after the `CONNECT` / `OK` handshake, so a quiet but
valid long-running exec can still complete.

Tests:

- `crates/m80-vsock/tests/frame_round_trip.rs::recv_raw_with_deadline_expires_during_slow_drip_frame`
- `crates/m80-firecracker/src/lifecycle/exec_tests.rs::recv_raw_for_exec_returns_host_timeout_on_slow_drip`
- `crates/m80-firecracker/src/lifecycle/exec_tests.rs::host_exec_deadline_preserves_requested_budget`
