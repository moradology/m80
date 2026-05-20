# Privileged Network Operation Battery

## Behavior

The Layer 2 network battery runs `m80-attack-runner` through the official
Firecracker jailer harness and verifies that privileged network operations are
blocked by the jailer/hardening boundary. Under the current compromised-VMM
network policy, the battery does not assert that ordinary TCP listen/connect
operations fail; `NoEgress` and `AllowOutbound` are guest-networking policies,
not private VMM network namespace guarantees.

Covered attack names:

- `open_raw_socket`: attempts `socket(AF_INET, SOCK_RAW, IPPROTO_RAW)`.
- `raw_packet_inject`: attempts `socket(AF_PACKET, SOCK_RAW, ETH_P_ALL)`.
- `send_arbitrary_netlink`: attempts an acknowledged `RTM_NEWLINK` mutation.
- `bind_on_host_interface`: attempts to bind a TEST-NET-3 nonlocal address.
- `privileged_route_mutation`: attempts an acknowledged `RTM_NEWROUTE`
  mutation.

Each attack exits `0` only if the forbidden operation succeeds. Expected kernel
denials return a non-zero exit code, which the ignored root-only harness treats
as blocked.

## Evidence

- `crates/m80-attack-runner/src/attacks/network.rs` owns the network attack
  primitives.
- `crates/m80-attack-runner/src/catalog.rs` registers the stable attack names.
- `crates/m80-jailer/tests/defense_in_depth.rs` has one ignored root-only
  harness test per attack.
- `docs/behaviors/security/compromised-vmm-network-boundary.md` explains why
  ordinary TCP probes are not part of this battery.

## Verification

Non-root/default verification compiles the ignored harness and attack runner:

- `cargo test -p m80-attack-runner --features malicious-artifact`
- `cargo test -p m80-jailer --test defense_in_depth --no-run`
- `cargo clippy -p m80-attack-runner --features malicious-artifact --all-targets -- -D warnings`
- `cargo clippy -p m80-jailer --test defense_in_depth -- -D warnings`

Full execution requires root, the official Firecracker jailer,
`m80-jailer-harden`, and the musl `m80-attack-runner` artifact.
