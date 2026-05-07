# OOM Event Surface Decision

Behavior bead: `m80-3xwa.5.6`.

## Decision

m80 does not add an asynchronous guest-to-host OOM push channel in
`m80-guestd` in the current protocol.

The current wire shape is deliberately request/response over a sequential
vsock connection. Adding an unsolicited `OOMEvent` frame would create a second
message direction with its own lifetime, buffering, reconnect, backpressure,
and teardown semantics. That is a larger transport contract than a single
guestd verb.

## Current Boundary

OOM enforcement for the VM belongs to the host cgroup surface. m80 already
creates and owns the VM process subtree outside the guest. A whole-VM OOM is
observable there without relying on an in-guest daemon surviving the memory
event it is supposed to report.

`m80-guestd` continues to expose guest-local counters through
`MetricsRequest` / `MetricsResponse`, but it does not promise an async event
stream.

## Future Shape

If m80 needs first-class OOM events, the safe shape is one of these hard
cutovers:

- a host-side `m80-cgroup` OOM event reader that records VM lifecycle evidence
  without touching the guest protocol;
- a pollable guest verb, for example `OomStatusRequest`, if a guest-local
  counter is needed and cgroup files exist inside the guest;
- a new explicit event-stream transport with bounded queues, reconnect rules,
  and teardown semantics that can carry more than OOM events.

Until one of those contracts is chosen, `m80-guestd` must not emit unsolicited
frames on the application request/response channel.
