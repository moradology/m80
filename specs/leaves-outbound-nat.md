# L1-11 Networking — OutboundNat (leaves)

This file captures all 38 leaf beads for the OutboundNat L1 epic, organized
under the seven L2 sub-epics. Every leaf is `$ACTIVE`; specification lives in
v0.1, while implementation is deferred to v0.2 per the plan.

Source-of-truth references:
- m80 dossier `06-network-internals.md` (full file)
- predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` (3669 LOC)
- predecessor `docs/gates/stage-g-firecracker-outbound-network-contract.md`

The predecessor module name `LEGACY_RULE_COMMENT_PREFIX` and the systemd unit
files `10-predecessor-outbound.network` / `10-predecessor-dns.conf` are renamed for m80
to `M80_RULE_COMMENT_PREFIX` and `10-m80-outbound.network` / `10-m80-dns.conf`.

## L2-11.1 Address allocation (parent_var: $L2_11_1)

### Leaf: Derive bridge name from sha256 of run_root path
- parent_var: $L2_11_1
- labels: $ACTIVE,network,outbound-nat,allocation
- status: open
- behavior: The system computes `bridge_name = "brfc" + first 12 hex chars of sha256(run_root_path)`, yielding a Linux interface name within the 15-byte limit and stable across restarts of the same run-root.
- source: dossier `06-network-internals.md` § Phase 1: planning; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `plan_outbound_nat_vm_network` lines 224-289 (specifically line 245).
- captured-by: m80/docs/behaviors/network-outbound-nat/address-allocation.md#bridge-name + m80/crates/network/tests/network-outbound-nat/address-allocation.rs::bridge_name_is_brfc_plus_12_hex_of_run_root_digest

### Leaf: Derive bridge CIDR from sha256 bytes of run_root
- parent_var: $L2_11_1
- labels: $ACTIVE,network,outbound-nat,allocation
- status: open
- behavior: The system derives the bridge CIDR as `172.<o2>.<o3>.0/24` where `o2 = (sha256(run_root)[0] % 16) + 16` (range 16..31) and `o3 = sha256(run_root)[1]` (range 0..255), with gateway pinned to `172.<o2>.<o3>.1`.
- source: dossier `06-network-internals.md` § Address allocation deep dive; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 246-249.
- captured-by: m80/docs/behaviors/network-outbound-nat/address-allocation.md#bridge-cidr + m80/crates/network/tests/network-outbound-nat/address-allocation.rs::bridge_cidr_is_172_o2_o3_slash_24_with_octet_formula

### Leaf: Cap bridge address space at 172.16.0.0/12
- parent_var: $L2_11_1
- labels: $ACTIVE,network,outbound-nat,allocation
- status: open
- behavior: The system constrains all bridge CIDRs to live within `172.16.0.0/12` (since `o2 ∈ [16,31]` and `o3 ∈ [0,255]`), giving a per-host capacity of `16 * 256 = 4096` non-colliding bridges before deterministic collisions become possible.
- source: dossier `06-network-internals.md` § Address allocation deep dive; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 246-248.
- captured-by: m80/docs/behaviors/network-outbound-nat/address-allocation.md#bridge-cidr-cap + m80/crates/network/tests/network-outbound-nat/address-allocation.rs::bridge_cidrs_stay_within_172_16_slash_12

### Leaf: Derive tap name from sha256 of (run_root, vm_id)
- parent_var: $L2_11_1
- labels: $ACTIVE,network,outbound-nat,allocation
- status: open
- behavior: The system computes `tap_name = "tfc" + first 12 hex chars of sha256(run_root_path || vm_id)`, deterministic per `(run_root, vm_id)` pair and within the 15-byte Linux interface name limit.
- source: dossier `06-network-internals.md` § Phase 1: planning; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 251-253.
- captured-by: m80/docs/behaviors/network-outbound-nat/address-allocation.md#tap-name + m80/crates/network/tests/network-outbound-nat/address-allocation.rs::tap_name_is_tfc_plus_12_hex_of_vm_digest

### Leaf: Derive guest IPv4 host octet from vm_digest bytes 5-6
- parent_var: $L2_11_1
- labels: $ACTIVE,network,outbound-nat,allocation
- status: open
- behavior: The system derives the guest IPv4 as `bridge_cidr.host(2 + (u16_be(vm_digest[5], vm_digest[6]) % 253))`, placing it in the per-bridge host range `[2, 254]` and making it deterministic per `(run_root, vm_id)`.
- source: dossier `06-network-internals.md` § Address allocation deep dive (Guest IP); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 254-255.
- captured-by: m80/docs/behaviors/network-outbound-nat/address-allocation.md#guest-ipv4 + m80/crates/network/tests/network-outbound-nat/address-allocation.rs::guest_ipv4_is_deterministic_per_run_root_and_vm_id

### Leaf: Derive guest MAC from vm_digest bytes 0-4 with locally-administered bit
- parent_var: $L2_11_1
- labels: $ACTIVE,network,outbound-nat,allocation
- status: open
- behavior: The system formats `guest_mac = "02:" + hex(vm_digest[0:5])` so that the first octet `0x02` sets the locally-administered bit and clears the multicast bit, yielding a unicast locally-administered MAC unique per `(run_root, vm_id)`.
- source: dossier `06-network-internals.md` § Address allocation deep dive (Guest MAC); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 258-265 and `validate_mac` lines 761-785.
- captured-by: m80/docs/behaviors/network-outbound-nat/address-allocation.md#guest-mac + m80/crates/network/tests/network-outbound-nat/address-allocation.rs::guest_mac_is_locally_administered_unicast

## L2-11.2 Collision detection (parent_var: $L2_11_2)

### Leaf: Reject guest IPv4 collision with sibling VM network state
- parent_var: $L2_11_2
- labels: $ACTIVE,network,outbound-nat,collision
- status: open
- behavior: The system scans `<run_root>/*/network-state.json` for any existing VM whose `bridge.cidr` matches the planned CIDR and whose `guest_ipv4` matches the planned guest IPv4 with a different `vm_id`, and fails closed with `NetworkAllocationConflict` if any are found.
- source: dossier `06-network-internals.md` § Phase 2: collision detection; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `reject_guest_ipv4_collision` lines 415-454.
- captured-by: m80/docs/behaviors/network-outbound-nat/collision.md#guest-ipv4 + m80/crates/network/tests/network-outbound-nat/collision.rs::reject_guest_ipv4_collision_with_sibling

### Leaf: Reject host route collision against /proc/net/route
- parent_var: $L2_11_2
- labels: $ACTIVE,network,outbound-nat,collision
- status: open
- behavior: The system reads `/proc/net/route`, parses the kernel route table, and fails closed with `NetworkCidrCollision` if the planned bridge CIDR overlaps any existing non-default route on an interface other than the (optionally) allowed bridge interface.
- source: dossier `06-network-internals.md` § Phase 2: collision detection; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `reject_host_route_collision` lines 456-480 and `parse_proc_net_route` lines 538-567.
- captured-by: m80/docs/behaviors/network-outbound-nat/collision.md#host-route + m80/crates/network/tests/network-outbound-nat/collision.rs::reject_host_route_collision_against_proc_net_route

### Leaf: Treat collision as hard error with no random fallback
- parent_var: $L2_11_2
- labels: $ACTIVE,network,outbound-nat,collision
- status: open
- behavior: The system never retries with a randomized address on collision; either guest-IPv4 or host-route collision short-circuits `prepare_vm_network` and surfaces a typed error to the caller, preserving deterministic-allocation invariants.
- source: dossier `06-network-internals.md` § Phase 2: collision detection ("If collision: hard error. No random fallback."); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 212-220 (caller path) and 444-450 (error variant).
- captured-by: m80/docs/behaviors/network-outbound-nat/collision.md#hard-error + m80/crates/network/tests/network-outbound-nat/collision.rs::collision_is_hard_error_no_fallback

## L2-11.3 Bridge & tap setup (parent_var: $L2_11_3)

### Leaf: Skip bridge realization when ownership state matches
- parent_var: $L2_11_3
- labels: $ACTIVE,network,outbound-nat,setup
- status: open
- behavior: The system treats `ensure_bridge_ready` as idempotent: when the bridge interface already exists and the on-disk `outbound-bridge-state.json` matches the planned identity (run_root_digest, bridge_name, cidr, gateway) and is in `Ready` phase, it verifies the IPv4 address and returns without re-issuing `ip link add`.
- source: dossier `06-network-internals.md` § Phase 3: bridge setup; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `ensure_bridge_ready` lines 291-347 and `validate_expected_bridge_state` lines 709-724.
- captured-by: m80/docs/behaviors/network-outbound-nat/setup.md#bridge-idempotency + m80/crates/network/tests/network-outbound-nat/setup.rs::bridge_setup_is_idempotent_with_matching_state

### Leaf: Persist outbound-bridge-state.json at run-root level
- parent_var: $L2_11_3
- labels: $ACTIVE,network,outbound-nat,setup
- status: open
- behavior: The system writes `outbound-bridge-state.json` at `<run_root>/outbound-bridge-state.json` via atomic write, recording `schema_version=1`, `setup_phase`, `run_root`, `run_root_digest`, `bridge_name`, `cidr`, and `gateway_ipv4`, with a Planned-then-Ready phase transition bracketing the `ip` invocations.
- source: dossier `06-network-internals.md` § State types and § Phase 3; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` constants line 23 and types lines 101-109; `write_bridge_state` lines 612-622.
- captured-by: m80/docs/behaviors/network-outbound-nat/setup.md#bridge-state-file + m80/crates/network/tests/network-outbound-nat/setup.rs::bridge_state_file_is_atomic_and_at_run_root

### Leaf: Create tap device via ip tuntap then attach to bridge
- parent_var: $L2_11_3
- labels: $ACTIVE,network,outbound-nat,setup
- status: open
- behavior: The system creates the per-VM tap by issuing `ip tuntap add dev <tap> mode tap`, then `ip link set dev <tap> address <guest_mac>`, then `ip link set dev <tap> master <bridge>`, and finally `ip link set dev <tap> up`, in that exact order.
- source: dossier `06-network-internals.md` § Phase 4: tap setup; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `ensure_tap_ready` lines 349-413.
- captured-by: m80/docs/behaviors/network-outbound-nat/setup.md#tap-creation + m80/crates/network/tests/network-outbound-nat/setup.rs::tap_creation_via_ip_tuntap_in_order

### Leaf: Persist per-VM network-state.json with atomic write
- parent_var: $L2_11_3
- labels: $ACTIVE,network,outbound-nat,setup
- status: open
- behavior: The system writes `<run_dir>/network-state.json` via atomic temp-file rename, recording `schema_version=1`, `setup_phase`, `vm_id`, `run_dir`, embedded `bridge` state, `iface_id="eth0"`, `tap_name`, `guest_mac`, `guest_ipv4`, `private_ipv4_exceptions`, `dns_resolvers`, and `runtime_rootfs_configured`, with the Ready phase written only after `ip` operations succeed.
- source: dossier `06-network-internals.md` § Phase 4 and § State types; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` types lines 111-125 and `write_vm_network_state` lines 624-634.
- captured-by: m80/docs/behaviors/network-outbound-nat/setup.md#vm-state-file + m80/crates/network/tests/network-outbound-nat/setup.rs::vm_network_state_is_atomic_with_phase_transition

### Leaf: Route every ip invocation through privileged-shim
- parent_var: $L2_11_3
- labels: $ACTIVE,network,outbound-nat,setup
- status: open
- behavior: The system routes all `ip link`, `ip addr`, `ip tuntap`, and `ip link delete` invocations through `run_privileged_host_command()`, which selects effective-root direct execution or `sudo -n` based on the host's privilege check at startup; no command bypasses the shim.
- source: dossier `06-network-internals.md` § Privilege model; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` host trait calls (e.g., lines 306-345, 365-411) routing through `foundation::run_privileged_host_command` lines 860-879.
- captured-by: m80/docs/behaviors/network-outbound-nat/setup.md#privileged-shim + m80/crates/network/tests/network-outbound-nat/setup.rs::all_ip_invocations_go_through_privileged_shim

## L2-11.4 Guest network injection (parent_var: $L2_11_4)

### Leaf: Discover DNS resolvers via resolvectl with /etc/resolv.conf fallback
- parent_var: $L2_11_4
- labels: $ACTIVE,network,outbound-nat,dns
- status: open
- behavior: The system invokes `resolvectl dns` first and parses tokens out of stdout; only if that command is missing, fails, or returns no admitted resolvers does it fall back to parsing `nameserver` lines from `/etc/resolv.conf`, and it errors with `NoUsableDnsResolvers` if both yield an empty admitted set.
- source: dossier `06-network-internals.md` § Phase 5 (DNS discovery); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `discover_dns_resolvers` lines 1413-1432, `parse_resolver_tokens` lines 1448-1456, and `parse_resolv_conf` lines 1434-1446.
- captured-by: m80/docs/behaviors/network-outbound-nat/dns.md#discovery + m80/crates/network/tests/network-outbound-nat/dns.rs::resolvectl_then_resolv_conf_fallback

### Leaf: Admit only public IPv4 resolvers via is_admitted_dns_resolver
- parent_var: $L2_11_4
- labels: $ACTIVE,network,outbound-nat,dns
- status: open
- behavior: The system passes every candidate resolver through `is_admitted_dns_resolver`, accepting only public unicast IPv4 addresses and rejecting any unspecified, loopback, private, link-local, multicast, broadcast, or documentation address.
- source: dossier `06-network-internals.md` § Phase 5 (DNS filter); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `is_admitted_dns_resolver` lines 1467-1489.
- captured-by: m80/docs/behaviors/network-outbound-nat/dns.md#admission + m80/crates/network/tests/network-outbound-nat/dns.rs::is_admitted_dns_resolver_admits_public_ipv4_only

### Leaf: Reject CGN, benchmark, and reserved-range resolver addresses
- parent_var: $L2_11_4
- labels: $ACTIVE,network,outbound-nat,dns
- status: open
- behavior: The system additionally rejects resolver candidates in carrier-grade NAT `100.64.0.0/10`, benchmark `198.18.0.0/15`, leading-zero `0.0.0.0/8`, and reserved/multicast `224.0.0.0/3` ranges beyond the standard Rust `Ipv4Addr` predicates, ensuring no implicit widening of the admitted public set.
- source: dossier `06-network-internals.md` § Phase 5 (rejects private/link-local/loopback/CGN/doc-ranges/multicast); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 1479-1487.
- captured-by: m80/docs/behaviors/network-outbound-nat/dns.md#reject-categories + m80/crates/network/tests/network-outbound-nat/dns.rs::reject_cgn_benchmark_reserved_resolvers

### Leaf: Inject 10-m80-outbound.network into per-VM runtime rootfs clone
- parent_var: $L2_11_4
- labels: $ACTIVE,network,outbound-nat,injection
- status: open
- behavior: The system writes a systemd-networkd unit file at `/etc/systemd/network/10-m80-outbound.network` inside the per-VM runtime rootfs ext4 clone (via `debugfs`), containing `[Match] MACAddress=<guest_mac>`, `[Network] Address=<guest_ipv4>/<prefix>`, `Gateway=<gateway>`, `DNS=<resolver>` lines, plus `IPv6AcceptRA=no` and `LinkLocalAddressing=no`.
- source: dossier `06-network-internals.md` § Phase 5 (rename unit prefix for m80); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` constant line 28 (`LEGACY_NETWORKD_FILE`), `build_guest_network_config` lines 1381-1411, and `write_guest_network_config` lines 2020-2029.
- captured-by: m80/docs/behaviors/network-outbound-nat/injection.md#networkd-unit + m80/crates/network/tests/network-outbound-nat/injection.rs::networkd_unit_injected_with_static_ip_and_dns

### Leaf: Inject 10-m80-dns.conf resolved drop-in into runtime rootfs clone
- parent_var: $L2_11_4
- labels: $ACTIVE,network,outbound-nat,injection,dns
- status: open
- behavior: The system writes a systemd-resolved drop-in at `/etc/systemd/resolved.conf.d/10-m80-dns.conf` inside the per-VM runtime rootfs ext4 clone, containing `[Resolve]`, `DNS=<space-separated admitted resolvers>`, `FallbackDNS=` (empty), and `Domains=~.` to route all guest DNS through the admitted upstream set.
- source: dossier `06-network-internals.md` § Phase 5 (resolved drop-in); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` constant line 29 (`LEGACY_RESOLVED_FILE`), `build_guest_network_config` lines 1409 and `write_guest_network_config` lines 2020-2029.
- captured-by: m80/docs/behaviors/network-outbound-nat/injection.md#resolved-dropin + m80/crates/network/tests/network-outbound-nat/injection.rs::resolved_dropin_injected_with_admitted_dns

### Leaf: Keep guest daemon out of host-driven networking configuration
- parent_var: $L2_11_4
- labels: $ACTIVE,network,outbound-nat,injection
- status: open
- behavior: The system relies entirely on `systemd-networkd` and `systemd-resolved` (started inside the guest by the unmodified base image) to apply the injected configuration; the m80 guest daemon never touches `/etc/systemd/network`, `/etc/resolv.conf`, or interface state at runtime.
- source: dossier `06-network-internals.md` § Phase 5 and § Image neutrality (gates contract §6); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 1022-1051 (host-side write only) and stage-G contract § 6 "Image Neutrality And Runtime Injection".
- captured-by: m80/docs/behaviors/network-outbound-nat/injection.md#image-neutrality + m80/crates/network/tests/network-outbound-nat/injection.rs::guest_daemon_does_not_touch_networking

## L2-11.5 iptables policy (parent_var: $L2_11_5)

### Leaf: Enable IPv4 forwarding via sysctl before installing rules
- parent_var: $L2_11_5
- labels: $ACTIVE,network,outbound-nat,iptables
- status: open
- behavior: The system invokes `sysctl -w net.ipv4.ip_forward=1` through the privileged shim before any iptables NAT or FORWARD rule is installed; failure to set the sysctl aborts policy realization.
- source: dossier `06-network-internals.md` § Phase 6 (NAT policy); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `ensure_ipv4_forwarding` lines 1491-1497 (called from `apply_outbound_nat_policy_with_host` line 1091).
- captured-by: m80/docs/behaviors/network-outbound-nat/iptables.md#ip-forward + m80/crates/network/tests/network-outbound-nat/iptables.rs::sysctl_ip_forward_set_before_rules

### Leaf: Create per-VM filter chain tfw + 12 hex of run_dir digest
- parent_var: $L2_11_5
- labels: $ACTIVE,network,outbound-nat,iptables
- status: open
- behavior: The system creates a per-VM iptables filter chain named `tfw + first 12 hex chars of sha256(run_dir)` via `iptables -w -t filter -N <chain>`, idempotently skipping creation when the chain already exists, so chain ownership maps deterministically to `(run_root, vm_id)`.
- source: dossier `06-network-internals.md` § Phase 6; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `outbound_nat_filter_chain` lines 1499-1501 and `ensure_iptables_chain` lines 1746-1780.
- captured-by: m80/docs/behaviors/network-outbound-nat/iptables.md#filter-chain + m80/crates/network/tests/network-outbound-nat/iptables.rs::per_vm_filter_chain_named_tfw_plus_12_hex

### Leaf: Accept UDP and TCP DNS to admitted resolvers in filter chain
- parent_var: $L2_11_5
- labels: $ACTIVE,network,outbound-nat,iptables,dns
- status: open
- behavior: The system appends one ACCEPT rule per admitted resolver for `-p udp -d <resolver> --dport 53` and a parallel one for `-p tcp -d <resolver> --dport 53` into the per-VM filter chain, each tagged with the per-VM rule comment.
- source: dossier `06-network-internals.md` § Phase 6 (DNS accept rules); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `ensure_outbound_nat_filter_chain_rules` lines 1519-1560.
- captured-by: m80/docs/behaviors/network-outbound-nat/iptables.md#dns-accept + m80/crates/network/tests/network-outbound-nat/iptables.rs::dns_accept_per_admitted_resolver_udp_and_tcp

### Leaf: Reject all other UDP and TCP traffic to port 53
- parent_var: $L2_11_5
- labels: $ACTIVE,network,outbound-nat,iptables,dns
- status: open
- behavior: After the per-resolver ACCEPT rules, the system appends two REJECT rules for `-p udp --dport 53` and `-p tcp --dport 53` into the per-VM filter chain, blocking guest DNS traffic to any non-admitted upstream.
- source: dossier `06-network-internals.md` § Phase 6 (DNS reject); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 1562-1581.
- captured-by: m80/docs/behaviors/network-outbound-nat/iptables.md#dns-reject + m80/crates/network/tests/network-outbound-nat/iptables.rs::reject_other_port_53

### Leaf: Accept bounded private IPv4 exceptions in filter chain
- parent_var: $L2_11_5
- labels: $ACTIVE,network,outbound-nat,iptables
- status: open
- behavior: For each normalized `OutboundNatPrivateIpv4Exception` (max `/24`, must overlap an admitted private/internal candidate class, must not overlap any permanent-deny class), the system appends an ACCEPT rule `-d <destination>` into the per-VM filter chain.
- source: dossier `06-network-internals.md` § Phase 6 (private exception accept rules) and gates contract § 4.1; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 1583-1600 and `validate_private_exception_destination` lines 913-949.
- captured-by: m80/docs/behaviors/network-outbound-nat/iptables.md#private-exceptions + m80/crates/network/tests/network-outbound-nat/iptables.rs::bounded_private_exceptions_accepted

### Leaf: Reject permanent-deny CIDR list in filter chain
- parent_var: $L2_11_5
- labels: $ACTIVE,network,outbound-nat,iptables
- status: open
- behavior: After private exceptions, the system appends REJECT rules for `0.0.0.0/8`, `10.0.0.0/8`, `100.64.0.0/10`, `127.0.0.0/8`, `169.254.0.0/16`, `172.16.0.0/12`, `192.168.0.0/16`, `192.0.2.0/24`, `198.18.0.0/15`, `198.51.100.0/24`, `203.0.113.0/24`, `224.0.0.0/4`, `240.0.0.0/4`, and the bridge CIDR itself (defense-in-depth) into the per-VM filter chain.
- source: dossier `06-network-internals.md` § Phase 6 (permanent-deny list) and gates contract § 4.1; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `denied_outbound_cidrs` lines 1721-1744.
- captured-by: m80/docs/behaviors/network-outbound-nat/iptables.md#permanent-deny + m80/crates/network/tests/network-outbound-nat/iptables.rs::permanent_deny_list_includes_bridge_cidr

### Leaf: Append default ACCEPT after deny list
- parent_var: $L2_11_5
- labels: $ACTIVE,network,outbound-nat,iptables
- status: open
- behavior: The system terminates the per-VM filter chain with a final default-ACCEPT rule (`-j ACCEPT` with the per-VM comment) so that any traffic surviving the DNS gate, private-exception accepts, and permanent-deny rejects reaches public IPv4 destinations.
- source: dossier `06-network-internals.md` § Phase 6 (default accept); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 1621-1627.
- captured-by: m80/docs/behaviors/network-outbound-nat/iptables.md#default-accept + m80/crates/network/tests/network-outbound-nat/iptables.rs::default_accept_after_deny_list

### Leaf: Insert FORWARD entries that route guest traffic through filter chain
- parent_var: $L2_11_5
- labels: $ACTIVE,network,outbound-nat,iptables
- status: open
- behavior: The system inserts (not appends) into the FORWARD chain three rules: `-i <tap> -s <guest_ipv4>/32 -j <chain>` to route guest-egress through the per-VM chain, `-o <tap> -d <guest_ipv4>/32 -j REJECT` to deny new inbound, and `-o <tap> -d <guest_ipv4>/32 -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT` to permit reply traffic.
- source: dossier `06-network-internals.md` § Phase 6 (forwarding entry); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `ensure_forwarding_entry_rules` lines 1630-1695.
- captured-by: m80/docs/behaviors/network-outbound-nat/iptables.md#forward-entries + m80/crates/network/tests/network-outbound-nat/iptables.rs::forward_inserts_route_guest_through_filter_chain

### Leaf: Append NAT POSTROUTING masquerade for guest source IPv4
- parent_var: $L2_11_5
- labels: $ACTIVE,network,outbound-nat,iptables
- status: open
- behavior: The system appends `-t nat -A POSTROUTING -s <guest_ipv4>/32 -j MASQUERADE` (tagged with the per-VM comment) so that guest egress is source-NATed onto whichever interface owns the host default route.
- source: dossier `06-network-internals.md` § Phase 6 (NAT masquerade); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `ensure_nat_masquerade_rule` lines 1697-1719.
- captured-by: m80/docs/behaviors/network-outbound-nat/iptables.md#nat-masquerade + m80/crates/network/tests/network-outbound-nat/iptables.rs::nat_postrouting_masquerade_for_guest_source

## L2-11.6 Rule tagging & teardown (parent_var: $L2_11_6)

### Leaf: Tag every owned iptables rule with M80_RULE_COMMENT_PREFIX
- parent_var: $L2_11_6
- labels: $ACTIVE,network,outbound-nat,iptables,cleanup
- status: open
- behavior: The system tags every owned iptables rule (filter chain entries, FORWARD inserts, NAT POSTROUTING) with `--comment "<M80_RULE_COMMENT_PREFIX>:<first 12 hex chars of sha256(run_root)>:<tap_name>"` so cleanup can identify owned rules without depending on chain naming.
- source: dossier `06-network-internals.md` § Phase 6 (rule tagging) and § Robustness; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` constant line 30 (`LEGACY_RULE_COMMENT_PREFIX`, renamed `M80_RULE_COMMENT_PREFIX` for m80) and `outbound_nat_rule_comment` lines 1503-1511.
- captured-by: m80/docs/behaviors/network-outbound-nat/teardown.md#rule-comment-prefix + m80/crates/network/tests/network-outbound-nat/teardown.rs::every_owned_rule_carries_per_vm_comment

### Leaf: Delete owned chain rules by exact comment match
- parent_var: $L2_11_6
- labels: $ACTIVE,network,outbound-nat,iptables,cleanup
- status: open
- behavior: During teardown the system lists chain rules via `iptables -w -t <table> -S <chain>` and deletes only those whose rule spec contains the per-VM comment; encountering an unowned (foreign) rule in an owned chain aborts cleanup with `NetworkAllocationConflict`.
- source: dossier `06-network-internals.md` § Teardown and recovery; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `delete_owned_iptables_chain_rules` lines 1819-1840 and `cleanup_outbound_nat_policy` lines 1202-1212.
- captured-by: m80/docs/behaviors/network-outbound-nat/teardown.md#cleanup-by-comment + m80/crates/network/tests/network-outbound-nat/teardown.rs::cleanup_deletes_only_rules_with_owned_comment

### Leaf: Delete per-VM filter chain only after it is empty
- parent_var: $L2_11_6
- labels: $ACTIVE,network,outbound-nat,iptables,cleanup
- status: open
- behavior: After deleting every comment-matched rule, the system runs `iptables -w -t filter -X <chain>` to delete the chain itself only when the chain has no remaining rules; any residue (owned or unowned) blocks chain deletion and surfaces a typed conflict.
- source: dossier `06-network-internals.md` § Teardown (chain delete); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `delete_iptables_chain_if_empty` lines 1842-1880.
- captured-by: m80/docs/behaviors/network-outbound-nat/teardown.md#chain-delete + m80/crates/network/tests/network-outbound-nat/teardown.rs::chain_deleted_only_when_empty

### Leaf: Tolerate repeated tap deletion via if-present wrappers
- parent_var: $L2_11_6
- labels: $ACTIVE,network,outbound-nat,cleanup
- status: open
- behavior: The system uses `delete_interface_if_present` (which probes via `ip link show dev <name>` and only issues `ip link delete dev <name>` when the interface exists), making `cleanup_vm_network` safe to call repeatedly on the same `vm_paths` without surfacing "Cannot find device" errors.
- source: dossier `06-network-internals.md` § Robustness (`delete_*_if_present` wrappers); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `delete_interface_if_present` lines 1301-1318 and `cleanup_vm_network_with_host` lines 1101-1121.
- captured-by: m80/docs/behaviors/network-outbound-nat/teardown.md#tap-delete-tolerant + m80/crates/network/tests/network-outbound-nat/teardown.rs::repeated_cleanup_calls_are_safe

### Leaf: Reject foreign rules discovered in owned filter chain
- parent_var: $L2_11_6
- labels: $ACTIVE,network,outbound-nat,iptables,cleanup
- status: open
- behavior: Before installing rules into a pre-existing per-VM chain, the system runs `reject_foreign_iptables_chain_rules`, which lists chain rules and fails closed with `NetworkAllocationConflict` if any rule line beyond the chain header lacks the expected per-VM comment, preventing accidental coexistence with foreign policy.
- source: dossier `06-network-internals.md` § Robustness; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `reject_foreign_iptables_chain_rules` lines 1782-1817.
- captured-by: m80/docs/behaviors/network-outbound-nat/teardown.md#foreign-rule-rejection + m80/crates/network/tests/network-outbound-nat/teardown.rs::foreign_rule_in_owned_chain_aborts

## L2-11.7 Bridge ownership & orphan recovery (parent_var: $L2_11_7)

### Leaf: Remove bridge only when no peer VM state references it
- parent_var: $L2_11_7
- labels: $ACTIVE,network,outbound-nat,cleanup
- status: open
- behavior: During `cleanup_vm_network`, the system scans `<run_root>/*/network-state.json` (excluding the current VM), and only when no sibling state file references the same `bridge.run_root` and `bridge.bridge_name` does it delete the bridge interface and remove `outbound-bridge-state.json`.
- source: dossier `06-network-internals.md` § Teardown and recovery; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `other_bridge_users_exist` lines 1320-1351 and `cleanup_vm_network_with_host` lines 1114-1118.
- captured-by: m80/docs/behaviors/network-outbound-nat/ownership.md#bridge-removal-guard + m80/crates/network/tests/network-outbound-nat/ownership.rs::bridge_removed_only_when_no_peer_references

### Leaf: Scavenge orphan bridge at startup when no VM states remain
- parent_var: $L2_11_7
- labels: $ACTIVE,network,outbound-nat,cleanup
- status: open
- behavior: When `cleanup_vm_network` is invoked on a VM whose `network-state.json` is missing, the system delegates to `cleanup_orphan_bridge_if_unused`, which probes the run-root for any sibling `network-state.json`; if none exist, it deletes the orphan bridge interface and removes `outbound-bridge-state.json`.
- source: dossier `06-network-internals.md` § Recovery at startup; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `cleanup_orphan_bridge_if_unused` lines 1123-1147 and `any_vm_network_states_exist` lines 1182-1200.
- captured-by: m80/docs/behaviors/network-outbound-nat/ownership.md#orphan-recovery + m80/crates/network/tests/network-outbound-nat/ownership.rs::startup_scavenges_orphan_bridge_when_unused

### Leaf: Tolerate malformed or missing state files during recovery
- parent_var: $L2_11_7
- labels: $ACTIVE,network,outbound-nat,cleanup
- status: open
- behavior: The system treats unreadable or unparseable peer state files as "live residue" (assuming the bridge is still in use rather than deleting), preserves ambiguous residue, and refuses to delete the bridge when its on-disk owner record disagrees with the cleanup VM's bridge identity.
- source: dossier `06-network-internals.md` § Recovery at startup ("tolerates missing/malformed state files; preserves ambiguous or live residue"); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 1341-1343 (Err returns true → preserve) and `validate_bridge_owner_record_for_cleanup` lines 1353-1362.
- captured-by: m80/docs/behaviors/network-outbound-nat/ownership.md#malformed-state-tolerance + m80/crates/network/tests/network-outbound-nat/ownership.rs::malformed_peer_state_preserves_bridge

### Leaf: Crash mid-VM does not break new VM startups
- parent_var: $L2_11_7
- labels: $ACTIVE,network,outbound-nat,cleanup
- status: open
- behavior: The system tolerates crash-mid-`prepare_vm_network` because deterministic naming plus owner-record validation make subsequent VM startups idempotent: a new VM either re-uses the existing bridge (when state matches), scavenges an orphan bridge during `cleanup_orphan_bridge_if_unused`, or surfaces a typed `NetworkAllocationConflict` instead of corrupting peer state.
- source: dossier `06-network-internals.md` § Robustness ("Crash recovery: next VM startup scavenges orphan bridges"); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `ensure_bridge_ready` lines 298-303 (existing-bridge path), `validate_expected_bridge_state` lines 709-724, and `cleanup_orphan_bridge_if_unused` lines 1123-1147.
- captured-by: m80/docs/behaviors/network-outbound-nat/ownership.md#crash-mid-vm + m80/crates/network/tests/network-outbound-nat/ownership.rs::crash_mid_vm_does_not_break_new_startup
