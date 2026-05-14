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
- These derivations are internal, documented, and pinned by in-crate tests.
  Implementations must reproduce them byte-for-byte.

### Realization

- `realize_bridge_and_tap(intent, vm_id, run_root, run_dir)` performs only
  the bridge/TAP setup phase. It writes bridge and per-VM network state
  atomically and uses the real link backend. Callers own parent directory
  creation; this function surfaces missing parents as I/O errors.
- The bridge is owned by the run-root, not by any single VM. Repeated
  realizations with the same `run_root` reuse the bridge if its
  recorded ownership matches; otherwise they fail closed with
  `BridgeOwnershipMismatch`. State lives in
  `<run_root>/outbound-bridge-state.json`.
- Before bridge/TAP mutation, setup rejects a planned bridge CIDR that overlaps
  a non-default host route, except for the route already owned by the same
  derived bridge interface.
- Bridge state is written in a Planned phase before host link mutation and
  in a Ready phase after bridge creation/address/up succeeds. A matching
  Ready state verifies the bridge address and skips bridge mutation. A
  matching Planned state is treated as crash-recovery input: if the bridge
  already exists, m80 completes address/up and promotes the state to Ready
  instead of trying to recreate the bridge.
- Per-VM state is written at `<run_dir>/network-state.json` in a Planned
  phase before TAP mutation and in a Ready phase after TAP creation, MAC
  assignment, bridge attach, and link-up succeed. Setup rejects existing
  planned guest IPv4 collisions before taking the allocation lock, then records
  a per-IP claim under that lock before writing Planned state. Concurrent
  same-IP launches fail closed without falling back to random addressing or
  scanning all sibling state files while the lock is held.
- If TAP setup fails after bridge creation, m80 deletes the partial TAP if
  present, removes the per-VM Planned state file, and scavenges the unused
  run-root bridge so failed launches do not strand owned network residue.
- Bridge creation, bridge address assignment, link MAC assignment, bridge
  attach/detach, link up/down, and link deletion use rtnetlink. The `ip`
  binary is not a runtime dependency for these operations.
- TAP creation uses the Linux TUN/TAP driver through a safe wrapper over
  `/dev/net/tun`; m80 then manages the resulting link through rtnetlink.
  This split is intentional: Linux `tun.c` allows rtnetlink deletion and
  introspection for TUN/TAP links, but not creation.
- `prepare_pid_one_network_cmdline(state)` is the current Ubuntu/Minimal
  PID-1 guest network path. It discovers admitted non-loopback IPv4 DNS resolvers,
  records `dns_resolvers` and `runtime_rootfs_configured=true` back to
  `<run_dir>/network-state.json`, and returns deterministic `m80.net.*`
  kernel command-line tokens for `m80-guestd` to consume after its overlay
  pivot.
- The old systemd-image injection path is retained internally for a future
  systemd-image mode. It discovers admitted non-loopback IPv4 DNS resolvers,
  writes the m80 systemd-networkd unit and systemd-resolved drop-in into a
  per-VM runtime rootfs ext4 image via `debugfs`, then records
  `dns_resolvers` and `runtime_rootfs_configured=true` back to
  `<run_dir>/network-state.json`.
- `apply_outbound_nat_policy(state)` performs only the host firewall
  phase. The VM and bridge state must both be Ready before policy
  installation begins, and guest network configuration must already have
  recorded at least one DNS resolver. It creates or reuses the
  deterministic per-VM filter chain, rejects foreign rules already present
  in that chain, sets `net.ipv4.ip_forward=1`, then lists the per-VM chain,
  FORWARD, and NAT POSTROUTING for missing-rule detection before installing
  missing rules through one `iptables-restore -w --noflush` batch. The
  batch appends per-VM filter rules, inserts bridge-interface FORWARD
  entries scoped by the guest `/32`, and appends NAT POSTROUTING masquerade.
- iptables rules are tagged with a per-VM comment prefix (rooted in
  `M80_RULE_COMMENT_PREFIX`). Cleanup finds rules by comment match —
  never by index — so concurrent rule additions by other tools don't
  break our teardown.
- The per-VM filter chain accepts configured DNS resolvers on UDP/TCP 53,
  rejects all other DNS, accepts bounded private exceptions, rejects the
  permanent-deny CIDR set, rejects IPv4 ICMP, then ends with default ACCEPT
  for public IPv4.
  The permanent-deny set is `0.0.0.0/8`, `10.0.0.0/8`,
  `100.64.0.0/10`, `127.0.0.0/8`, `169.254.0.0/16`,
  `172.16.0.0/12`, `192.168.0.0/16`, documentation/benchmark ranges,
  multicast/reserved ranges, and the specific bridge CIDR.
  Admitted DNS resolvers may be public or private LAN IPv4 addresses. Loopback,
  link-local, CGN, documentation, benchmark, multicast, and reserved addresses
  are rejected.

### Cleanup

- `cleanup_vm(vm_id: &str, run_root: &Path) -> Result<(), NetError>`
  removes only owned residue for that VM: exact comment-tagged FORWARD and
  NAT rules, comment-owned rules in the per-VM filter chain, the empty
  per-VM chain, the TAP link, the guest-IP claim, and the VM network state
  file. Repeated calls tolerate missing state and missing links. If the state
  file is missing, cleanup derives the TAP name and guest IP from
  `(run_root, vm_id)`, deletes owned residue, and then runs orphan bridge
  recovery.
- Cleanup aborts on foreign rules inside an owned per-VM filter chain. It
  never deletes unowned rules by index or broad match.
- `cleanup_orphan_bridge(run_root: &Path) -> Result<(), NetError>` is
  called at startup and removes the bridge only when no peer VM in the
  run-root references it. Ambiguity preserves residue.

## Public surface

- `realize_bridge_and_tap(...)` and
  `realize_bridge_and_tap_with_ops(...)` — bridge/TAP setup phase; the
  `_with_ops` variant is the deterministic test seam.
- `realize_bridge_and_tap_with_ops_for_routes(...)` — deterministic test seam
  for the same setup path with supplied `/proc/net/route` text.
- `apply_outbound_nat_policy(...)` and
  `apply_outbound_nat_policy_with_ops(...)` — host sysctl/iptables phase;
  the `_with_ops` variant is the deterministic command-recording seam.
- `discover_dns_resolvers_with_ops(...)` and
  `is_admitted_dns_resolver(...)` — DNS discovery seam and resolver-address
  admission helper.
- `prepare_pid_one_network_cmdline(...)` — current PID-1 guest
  network config preparation; discovers DNS, updates the network state file,
  and returns deterministic `m80.net.*` cmdline tokens.
- `PidOneNetworkCmdline { args }` — current PID-1 command-line token shape.
- `cleanup_vm(vm_id, run_root) -> Result<(), NetError>`.
- `cleanup_vm_with_ops(...)` — deterministic test seam for VM policy + TAP
  cleanup.
- `cleanup_outbound_nat_policy_with_ops(...)` — deterministic test seam for
  policy-only cleanup.
- `cleanup_orphan_bridge(run_root) -> Result<(), NetError>` and
  `cleanup_orphan_bridge_with_ops(...)` — orphan bridge recovery, with a
  deterministic test seam.
- `M80_RULE_COMMENT_PREFIX`, `outbound_nat_filter_chain(...)`,
  `outbound_nat_rule_comment(...)`, and `permanent_deny_cidrs(...)` —
  public deterministic helpers for policy identity and tests.
- `NETWORK_STATE_FILE` — per-VM network state filename under each VM run
  directory.
- `RealizedNetwork { bridge_name, tap_name, guest_ipv4, guest_mac,
  bridge_cidr }` — observable handles for diagnostics.
- `VmNetworkStateRecord` — opaque per-VM network state record returned by
  `read_vm_network_state_record(...)` and accepted by cmdline/policy helpers.
- `read_vm_network_state_record(...)` — read the opaque per-VM network state
  record from a VM run directory.
- `LinkOps` — bridge/TAP setup link-operation seam used by tests and the
  real internal rtnetlink/TUN backend.
- `PolicyOps` — host policy command seam used by tests and the real
  `sysctl`/`iptables`/`iptables-restore` backend. Implementors provide
  `command_output(...)` and `run_command_input(...)`; the default
  `run_command(...)` turns non-zero status into `NetError`.
- `PolicyCommandOutput { status_success, stdout, stderr }` with
  `success(...)` and `failure(...)` constructors — captured host policy
  command output.
- `DnsDiscoveryOps` — host seam for DNS discovery. Implementors provide
  `command_output(...)` and `read_to_string(...)`.
- `DnsCommandOutput { status_success, stdout, stderr }` with
  `success(...)` and `failure(...)` constructors — captured DNS helper
  command output.
- `NetError`: `Ipv6Unsupported`, `GuestIpv4Collision { peer_vm_id }`,
  `HostRouteCollision { existing }`, `BridgeOwnershipMismatch`,
  `NetworkCommandFailed { program, stderr }`,
  `NoUsableDnsResolvers`,
  `NetlinkOperationFailed { operation, detail }`,
  `TapOperationFailed { operation, source }`,
  `LinkNotFound { operation, name }`, `ForeignChainRule { rule }`,
  `NetworkAllocationConflict { path, detail }`,
  `InvalidNetworkState { path, detail }`,
  `PathIo { path, source }`, `Io(io::Error)`.

## Non-goals

- **No IPv6.** Rejected at `m80-net-mode`; reaffirmed here.
- **No nftables.** Even though it's the modern interface, iptables is
  what the contract pins. Adding nft is its own future epic.
- **No outbound-rate-limiting / QoS.** v0.1 is connectivity, not
  shaping.
- **No port forwarding.** Inbound is out of scope.
- **No DHCP.** Static IPv4 is supplied as PID-1 cmdline tokens for current
  images or systemd-networkd files for future systemd image mode; the host
  runs no DHCP server.
- **No DNS name allowlist yet.** The current policy admits DNS resolvers and
  enforces CIDR/IP rules. Domain allowlisting requires an explicit DNS proxy
  design; see `docs/behaviors/network-outbound-nat/dns-name-allowlist.md`.

## Dependencies

- `m80-net-mode` — callers pass `m80_net_mode::OutboundIntent` into the
  setup/policy helpers; `m80-net-outbound` does not re-export it.
- `sha2`, `hex`, `ipnet` — derivation math.
- `serde`, `serde_json` — state files.
- `nix` — process/file locking around guest-IP claim creation.
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
  produces `GuestIpv4Collision`; a barrier-synchronized setup test pins that
  concurrent colliding VM ids cannot both remain ready, and setup/cleanup tests
  pin guest-IP claim creation and removal.
- Host route collision: a fixture route overlapping the planned bridge CIDR
  produces `HostRouteCollision` before any link operation runs.
- Bridge/TAP setup: matching Ready bridge ownership skips bridge mutation,
  bridge and per-VM state files are written atomically with Planned/Ready
  phase transitions, ownership mismatch fails before link mutation, and a TAP
  setup failure after bridge creation rolls back bridge/VM state.
- Link-ops seam: tap/bridge lifecycle ordering is pinned without an `ip`
  shellout, and an ignored root/CAP_NET_ADMIN probe exercises real TAP
  create/delete through the no-`/sbin/ip` path.
- DNS discovery: tests pin `resolvectl dns` before `/etc/resolv.conf`
  fallback, public/private-LAN IPv4 admission, and rejection of loopback,
  link-local, CGN, documentation, benchmark, multicast, and reserved resolver
  addresses.
- Guest network injection: tests pin the current PID-1 cmdline tokens, DNS
  discovery/state updates, guest cmdline parsing, rtnetlink/resolv.conf
  operation ordering, and the retained systemd-image
  `10-m80-outbound.network` / `10-m80-dns.conf` writer.
- iptables policy: command-recording tests pin sysctl ordering, chain
  naming, DNS accept/reject rules, private exception accepts,
  permanent-deny rejects, default ACCEPT placement, FORWARD inserts, NAT
  masquerade, and idempotent reapply behavior.
- Bridge ownership/recovery: cleanup removes the shared bridge only after
  the last peer state is gone, startup scavenges orphan bridge state, malformed
  peer state preserves ambiguous residue, missing-state TAP orphans are deleted,
  and crash-mid-VM startup recovery is pinned.
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
- DNS admittance: every documented rejected resolver category (loopback,
  link-local, CGN, doc-ranges, benchmark, multicast, and reserved) is rejected;
  public and private LAN IPv4 resolvers are accepted.
- Foreign rule guard: pre-existing rules in our chain return
  `ForeignChainRule` rather than being silently coexisted with.
