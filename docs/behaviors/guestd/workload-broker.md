# Guestd Workload Broker Seam

Behavior capture for bead `m80-8emae.14.2`.

Buffered exec, streaming exec, and PTY exec all route workload command
construction through `crates/m80-guestd/src/workload_broker.rs`. The broker
seam owns the fixed workload launch surface: request-size validation, the
guest-root exec shim selection, process-group setup for stdio exec, and PTY
command construction. If the broker is poisoned, later workload requests fail
closed with `workload broker unavailable`.

This leaf intentionally preserves current spawn behavior. The follow-up
seccomp leaf can move the same seam behind a long-lived broker process and
install daemon/workload seccomp profiles without re-splitting buffered,
streaming, and PTY launch logic.

Tests:

- `crates/m80-guestd/tests/guestd/workload_broker.rs::buffered_exec_oversized_request_fails_at_broker_boundary`
- `crates/m80-guestd/tests/guestd/workload_broker.rs::streaming_exec_oversized_request_fails_at_broker_boundary`
- `crates/m80-guestd/tests/guestd/workload_broker.rs::pty_exec_oversized_request_fails_at_broker_boundary`
- `crates/m80-guestd/src/workload_broker.rs::tests::buffered_exec_routes_through_broker_seam`
- `crates/m80-guestd/src/workload_broker.rs::tests::streaming_exec_routes_through_broker_seam`
- `crates/m80-guestd/src/workload_broker.rs::tests::pty_exec_routes_through_broker_seam`
- `crates/m80-guestd/src/workload_broker.rs::tests::poisoned_broker_fails_closed`
- `crates/m80-guestd/src/workload_broker.rs::tests::oversized_broker_request_fails_before_spawn`
