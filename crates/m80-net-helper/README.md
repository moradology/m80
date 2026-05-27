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
  `M80_NET_HELPER_BIN` or `/opt/m80/bin/m80-net-helper`.
- `m80-firecracker` launches the helper according to
  `Discovery::chosen_launch_path`: directly on the wrapper path, or as one
  long-lived transient `systemd-run` unit per backend helper lifetime on the
  systemd path. The request/response protocol is identical on both paths.
- The systemd path grants the helper `CAP_NET_ADMIN` plus `CAP_SYS_ADMIN`.
  `CAP_SYS_ADMIN` is required because named network namespaces are implemented
  with bind mounts under `/run/netns`; filesystem-hardening directives that
  put the helper in a private mount namespace are not part of the helper
  envelope.
- `m80-net-helper --version` prints `m80-net-helper <package-version>` and
  exits without reading protocol input. `host-binaries.manifest.json` pins its
  path, sha256, and version.

## Public Surface

This crate exposes only the `m80-net-helper` binary. The protocol types and
stdio server live in `m80-net-outbound`. `--version` is reserved for
install-time identity capture and does not enter the stdio protocol loop.

## Tests

- `tests/version.rs` proves `--version` reports the package version without
  requiring protocol input.
