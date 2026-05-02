# `m80-net-outbound`

The whole `OutboundNat` story: deterministic addressing, bridge + tap
setup, guest network injection, default-deny iptables policy with
admitted DNS resolvers, ownership-aware idempotent cleanup. The largest
crate in the workspace (~3,500 LOC) and the one with the highest
operational risk.

## Reason for being

Egress networking for a sandbox is a security boundary, an
operational-debt magnet, and the largest single LOC contributor in
the upstream predecessor implementation (`network.rs` is 3,669 lines).
Sequestering it has three benefits:

1. **The rest of the codebase never touches `iptables`.** Anyone reading
   `m80-firecracker` or `m80-storage` doesn't need to know the rule
   ordering, the comment-tagging convention, or the chain naming scheme.
2. **The privilege surface is contained.** Only this crate shells out to
   `ip`/`iptables`/`sysctl` for network setup. The m80 process must already
   hold `CAP_NET_ADMIN` (or run as root); `m80-preflight` verifies at
   startup, so calls here use `Command::new` directly without a per-call
   privilege check.
3. **Network changes can be released independently.** Bumping the rule
   set, adjusting the CIDR cap, or fixing a teardown bug doesn't force
   a re-cert of the rest of the system.

This is also the crate that v0.1 *specifies* fully but defers
*implementing* — the dossier flags it as the largest extraction risk.
Having the spec in beads + the README contract here means a v0.2 author
has a clear starting point.

## Black-box contract

### Determinism

- Bridge name is `brfc` followed by the first 12 hex chars of
  `sha256(run_root_path)`.
- Bridge CIDR is `172.<o2>.<o3>.0/24` where
  `o2 = (sha256(run_root_path)[0] % 16) + 16` and
  `o3 = sha256(run_root_path)[1]`. The 172.16.0.0/12 envelope caps a
  single host at ~4096 collision-free bridges.
- Tap name is `tfc` followed by the first 12 hex chars of
  `sha256(run_root_path || vm_id)`.
- Guest IPv4 is derived deterministically from `(run_root, vm_id)`;
  guest MAC is `02:` + the first 5 bytes of the same digest, with the
  locally-administered bit set.
- These derivations are public, documented, and pinned by tests.
  Implementations must reproduce them byte-for-byte.

### Realization

- `realize(intent: &OutboundIntent, vm_id: &str, run_root: &Path) ->
  Result<RealizedNetwork, NetError>` performs the full Phase 1–6 pipeline:
  address allocation, collision detection (`reject_guest_ipv4_collision`,
  `reject_host_route_collision`), bridge + tap setup, guest network
  config injection into the VM's rootfs clone, iptables policy
  installation. Each phase is independently testable; the top-level call
  composes them.
- The bridge is owned by the run-root, not by any single VM. Repeated
  realizations with the same `run_root` reuse the bridge if its
  recorded ownership matches; otherwise they fail closed with
  `BridgeOwnershipMismatch`. State lives in
  `<run_root>/outbound-bridge-state.json`.
- iptables rules are tagged with a per-VM comment prefix (rooted in
  `M80_RULE_COMMENT_PREFIX`). Cleanup finds rules by comment match —
  never by index — so concurrent rule additions by other tools don't
  break our teardown.
- The default-deny set is: bridge CIDR, link-local (169.254/16),
  loopback (127/8), the host's known peer-VM subnets, and IPv6.
  Admitted DNS resolvers are public-IPv4 only (private/link-local/
  loopback/CGN/doc-ranges/multicast all rejected).

### Cleanup

- `cleanup_vm(vm_id: &str, run_root: &Path) -> Result<(), NetError>`
  removes only owned residue (rules with our comment, taps we created).
  It tolerates missing state files, partially-applied rules, and
  prior-crash residue.
- `cleanup_orphan_bridge(run_root: &Path) -> Result<(), NetError>` is
  called at startup and removes the bridge only when no peer VM in the
  run-root references it. Ambiguity preserves residue.

## Public surface

- `realize(&OutboundIntent, vm_id, run_root) -> Result<RealizedNetwork, NetError>`.
- `cleanup_vm(vm_id, run_root) -> Result<(), NetError>`.
- `cleanup_orphan_bridge(run_root) -> Result<(), NetError>`.
- `OutboundIntent` (re-exported from `m80-net-mode`).
- `RealizedNetwork { bridge_name, tap_name, guest_ipv4, guest_mac,
  bridge_cidr }` — observable handles for diagnostics.
- `derive_bridge_name(run_root) -> String` and friends — public,
  deterministic helpers.
- `NetError`: `Ipv6Unsupported`, `GuestIpv4Collision { peer_vm_id }`,
  `HostRouteCollision { existing }`, `BridgeOwnershipMismatch`,
  `IptablesCommandFailed { stderr }`, `IpCommandFailed { stderr }`,
  `ForeignChainRule { rule }`,
  `Io(io::Error)`.

## Non-goals

- **No IPv6.** Rejected at `m80-net-mode`; reaffirmed here.
- **No nftables.** Even though it's the modern interface, iptables is
  what the contract pins. Adding nft is its own future epic.
- **No outbound-rate-limiting / QoS.** v0.1 is connectivity, not
  shaping.
- **No port forwarding.** Inbound is out of scope.
- **No DHCP.** Static IPv4 is injected via systemd-networkd in the
  guest rootfs clone; the host runs no DHCP server.

## Dependencies

- `m80-net-mode` — for `OutboundIntent`.
- `sha2`, `hex`, `ipnet` — derivation math.
- `serde`, `serde_json` — state files.
- `thiserror`, `tracing`.

## Tests

- Determinism table: a fixed list of `(run_root, vm_id)` inputs maps to
  a known set of bridge/tap/IP/MAC outputs. Pinned in JSON.
- Collision detection: a fixture with two VMs claiming the same guest IP
  produces `GuestIpv4Collision`.
- Idempotent realize: calling `realize` twice with the same input
  produces the same iptables rule set (verified by `iptables-save`).
- Comment-tagged cleanup: `cleanup_vm` removes only rules matching our
  comment prefix, even when foreign rules exist in the same chains.
- Orphan-bridge recovery: a state file pointing at a missing bridge
  cleans up gracefully; an ambiguous state file is preserved.
- DNS admittance: every documented resolver category (private/link-local/
  loopback/CGN/doc-ranges/multicast) is rejected; a public IPv4 is
  accepted.
- Foreign rule guard: pre-existing rules in our chain return
  `ForeignChainRule` rather than being silently coexisted with.
