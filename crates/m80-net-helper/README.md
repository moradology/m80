# `m80-net-helper`

Privileged helper executable for m80-owned outbound network operations.

## Reason for Being

The long-lived m80 backend should not retain `CAP_NET_ADMIN` just because later
OutboundNat launch and cleanup paths may need bridge, namespace, veth, TAP,
sysctl, or iptables mutation. This binary is the narrow process boundary for
those operations.

## Black-Box Contract

- The helper reads newline-delimited JSON requests on stdin and writes one JSON
  response per request on stdout.
- The request enum is finite and owned by `m80-net-outbound`: realize one
  OutboundNat topology, apply one VM policy, clean one VM, or clean an orphan
  run-root bridge.
- Unknown operations and oversized request frames fail closed with a typed
  `invalid_request` response. The helper never accepts arbitrary commands.
- Operation failures return typed helper error frames with diagnostic text; the
  caller decides whether a failed launch or cleanup is fatal.
- The helper is a host TCB binary. `m80-preflight` discovers it through
  `M80_NET_HELPER_BIN` or `/opt/m80/bin/m80-net-helper`, and
  `host-binaries.manifest.json` pins its path and sha256.

## Public Surface

This crate exposes only the `m80-net-helper` binary. The protocol types and
stdio server live in `m80-net-outbound`.
