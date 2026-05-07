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
   `iptables`/`sysctl` for network policy. Bridge, address, link, and
   delete operations use rtnetlink directly; TAP creation goes through the
   Linux TUN/TAP driver because the kernel rejects TUN/TAP creation over
   rtnetlink. The m80 process must already hold `CAP_NET_ADMIN` (or run as
   root); `m80-preflight` verifies at startup, so calls here use the host
   kernel APIs directly without a per-call privilege check.
3. **Network changes can be released independently.** Bumping the rule
   set, adjusting the CIDR cap, or fixing a teardown bug doesn't force
   a re-cert of the rest of the system.

## Black-box contract

### Determinism

- Bridge name is `brfc` followed by the first 11 hex chars of
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
  Result<RealizedNetwork, NetError>` performs the full Phase 1–5 pipeline:
  (1) address allocation, (2) collision detection
  (`reject_guest_ipv4_collision`, `reject_host_route_collision`),
  (3) bridge + tap setup, (4) guest network config injection into the VM's
  rootfs clone, (5) iptables policy installation. Each phase is
  independently testable; the top-level call composes them. **v0.1 status:**
  the pipeline is not yet wired in `realize`; callers invoke the phase
  functions directly.
- `realize_bridge_and_tap(intent, vm_id, run_root, run_dir)` performs only
  the bridge/TAP setup phase. It writes bridge and per-VM network state
  atomically and uses the real link backend. Callers own parent directory
  creation; this function surfaces missing parents as I/O errors.
- The bridge is owned by the run-root, not by any single VM. Repeated
  realizations with the same `run_root` reuse the bridge if its
  recorded ownership matches; otherwise they fail closed with
  `BridgeOwnershipMismatch`. State lives in
  `<run_root>/outbound-bridge-state.json`.
- Bridge state is written in a Planned phase before host link mutation and
  in a Ready phase after bridge creation/address/up succeeds. A matching
  Ready state verifies the bridge address and skips bridge mutation.
- Per-VM state is written at `<run_dir>/network-state.json` in a Planned
  phase before TAP mutation and in a Ready phase after TAP creation, MAC
  assignment, bridge attach, and link-up succeed.
- Bridge creation, bridge address assignment, link MAC assignment, bridge
  attach/detach, link up/down, and link deletion use rtnetlink. The `ip`
  binary is not a runtime dependency for these operations.
- TAP creation uses the Linux TUN/TAP driver through a safe wrapper over
  `/dev/net/tun`; m80 then manages the resulting link through rtnetlink.
  This split is intentional: Linux `tun.c` allows rtnetlink deletion and
  introspection for TUN/TAP links, but not creation.
- `inject_guest_network_config(state, runtime_rootfs)` discovers admitted
  public IPv4 DNS resolvers, writes the m80 systemd-networkd unit and
  systemd-resolved drop-in into the supplied per-VM runtime rootfs ext4
  image via `debugfs`, then records `dns_resolvers` and
  `runtime_rootfs_configured=true` back to `<run_dir>/network-state.json`.
- `apply_outbound_nat_policy(state)` performs only the host firewall
  phase. The VM and bridge state must both be Ready before policy
  installation begins, and guest network configuration must already have
  recorded at least one DNS resolver. It creates or reuses the
  deterministic per-VM filter chain, rejects foreign rules already present
  in that chain, sets `net.ipv4.ip_forward=1`, appends the per-VM filter
  rules, inserts the FORWARD entries, and appends NAT POSTROUTING
  masquerade.
- iptables rules are tagged with a per-VM comment prefix (rooted in
  `M80_RULE_COMMENT_PREFIX`). Cleanup finds rules by comment match —
  never by index — so concurrent rule additions by other tools don't
  break our teardown.
- The per-VM filter chain accepts configured DNS resolvers on UDP/TCP 53,
  rejects all other DNS, accepts bounded private exceptions, rejects the
  permanent-deny CIDR set, then ends with default ACCEPT for public IPv4.
  The permanent-deny set is `0.0.0.0/8`, `10.0.0.0/8`,
  `100.64.0.0/10`, `127.0.0.0/8`, `169.254.0.0/16`,
  `172.16.0.0/12`, `192.168.0.0/16`, documentation/benchmark ranges,
  multicast/reserved ranges, and the specific bridge CIDR.
  Admitted DNS resolvers are public-IPv4 only (private/link-local/
  loopback/CGN/doc-ranges/multicast all rejected).

### Cleanup

- `cleanup_vm(vm_id: &str, run_root: &Path) -> Result<(), NetError>`
  removes only owned residue for that VM: exact comment-tagged FORWARD and
  NAT rules, comment-owned rules in the per-VM filter chain, the empty
  per-VM chain, the TAP link, and the VM network state file. Repeated calls
  tolerate missing state and missing links.
- Cleanup aborts on foreign rules inside an owned per-VM filter chain. It
  never deletes unowned rules by index or broad match.
- `cleanup_orphan_bridge(run_root: &Path) -> Result<(), NetError>` is
  called at startup and removes the bridge only when no peer VM in the
  run-root references it. Ambiguity preserves residue.

## Public surface

- `realize(&OutboundIntent, vm_id, run_root) -> Result<RealizedNetwork, NetError>`.
- `realize_bridge_and_tap(...)` and
  `realize_bridge_and_tap_with_ops(...)` — bridge/TAP setup phase; the
  `_with_ops` variant is the deterministic test seam.
- `apply_outbound_nat_policy(...)` and
  `apply_outbound_nat_policy_with_ops(...)` — host sysctl/iptables phase;
  the `_with_ops` variant is the deterministic command-recording seam.
- `discover_dns_resolvers(...)`, `discover_dns_resolvers_with_ops(...)`,
  and `is_admitted_dns_resolver(...)` — DNS discovery and public-IPv4
  admission helpers.
- `inject_guest_network_config(...)` and
  `inject_guest_network_config_with_ops(...)` — host-driven guest
  networkd/resolved file injection into a runtime ext4 image; the
  `_with_ops` variant is the deterministic debugfs/DNS seam.
- `cleanup_vm(vm_id, run_root) -> Result<(), NetError>`.
- `cleanup_vm_with_ops(...)` — deterministic test seam for VM policy + TAP
  cleanup.
- `cleanup_outbound_nat_policy_with_ops(...)` — deterministic test seam for
  policy-only cleanup.
- `cleanup_orphan_bridge(run_root) -> Result<(), NetError>`.
- `OutboundIntent` (re-exported from `m80-net-mode`).
- `M80_RULE_COMMENT_PREFIX`, `outbound_nat_filter_chain(...)`,
  `outbound_nat_rule_comment(...)`, and `permanent_deny_cidrs(...)` —
  public deterministic helpers for policy identity and tests.
- `SYSTEMD_NETWORK_DIR`, `SYSTEMD_RESOLVED_CONF_DIR`,
  `M80_NETWORKD_FILE`, and `M80_RESOLVED_FILE` — guest config paths
  written by the injection phase.
- `RealizedNetwork { bridge_name, tap_name, guest_ipv4, guest_mac,
  bridge_cidr }` — observable handles for diagnostics.
- `GuestNetworkConfig { networkd, resolved }` — rendered contents for the
  guest systemd files.
- `derive_bridge_name(run_root) -> String` and friends — public,
  deterministic helpers.
- `BridgeState`, `VmNetworkStateRecord`, `SetupPhase`, and
  `planned_*`/`read_*`/`write_*` helpers for the bridge and per-VM network
  state files.
- `LinkOps` — bridge/TAP setup link-operation seam used by tests and the
  real rtnetlink/TUN backend.
- `PolicyOps` and `PolicyCommandOutput` — host policy command seam used
  by tests and the real `sysctl`/`iptables` backend.
- `DnsDiscoveryOps`, `DnsCommandOutput`, and `GuestNetworkConfigOps` —
  host seams for DNS discovery and debugfs-backed guest config writes.
- `reject_guest_ipv4_collision(...)` and
  `reject_host_route_collision(...)` — fail-closed collision checks for the
  pure planning phase.
- `NetError`: `Ipv6Unsupported`, `GuestIpv4Collision { peer_vm_id }`,
  `HostRouteCollision { existing }`, `BridgeOwnershipMismatch`,
  `NetworkCommandFailed { program, stderr }`,
  `NoUsableDnsResolvers`,
  `NetlinkOperationFailed { operation, detail }`,
  `TapOperationFailed { operation, source }`,
  `LinkNotFound { operation, name }`, `ForeignChainRule { rule }`,
  `NetworkAllocationConflict { path, detail }`,
  `InvalidNetworkState { path, detail }`, `Io(io::Error)`.

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
- `rtnetlink`, `futures-util`, `tokio` — direct netlink control for
  bridge/address/link operations.
- `tun` — safe TAP creation over the Linux TUN/TAP driver.
- `tempfile` — temporary host files used for debugfs-backed guest config
  injection.
- `thiserror`, `tracing`.

## Tests

- Determinism table: a fixed list of `(run_root, vm_id)` inputs maps to
  a known set of bridge/tap/IP/MAC outputs. Pinned in JSON.
- Collision detection: a fixture with two VMs claiming the same guest IP
  produces `GuestIpv4Collision`.
- Bridge/TAP setup: matching Ready bridge ownership skips bridge mutation,
  bridge and per-VM state files are written atomically with Planned/Ready
  phase transitions, and ownership mismatch fails before link mutation.
- Link-ops seam: tap/bridge lifecycle ordering is pinned without an `ip`
  shellout, and an ignored root/CAP_NET_ADMIN probe exercises real TAP
  create/delete through the no-`/sbin/ip` path.
- DNS discovery: tests pin `resolvectl dns` before `/etc/resolv.conf`
  fallback, public-IPv4 admission, and rejection of private/link-local/
  loopback/CGN/documentation/benchmark/reserved resolver addresses.
- Guest network injection: tests pin the rendered `10-m80-outbound.network`
  and `10-m80-dns.conf` contents, debugfs directory/write calls, network
  state updates, and the absence of guest-daemon networking writes.
- iptables policy: command-recording tests pin sysctl ordering, chain
  naming, DNS accept/reject rules, private exception accepts,
  permanent-deny rejects, default ACCEPT placement, FORWARD inserts, NAT
  masquerade, and idempotent reapply behavior.
- Bridge ownership/recovery: cleanup removes the shared bridge only after
  the last peer state is gone, startup scavenges orphan bridge state, malformed
  peer state preserves ambiguous residue, and crash-mid-VM startup recovery is
  pinned.
- Teardown: command-recording tests pin per-VM comments on all owned rules,
  deletion by exact comment-owned rule specs, chain deletion only after
  empty, foreign-rule rejection, and repeated cleanup safety for missing TAP
  state.
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
