# 06 — `network.rs`: 3,669 lines of bridges, taps, NAT, and recovery

`crates/sandbox/agent-sandbox-firecracker/src/network.rs` is the largest
single file in the crate. It owns everything host-side about
`OutboundNat` mode — bridge creation, tap allocation, MAC/IP allocation,
iptables policy, DNS injection, teardown, and crash recovery.

This file is the largest single risk in the extraction. Plan for 1-2
weeks just for it.

## Module structure

### Configuration types (lines 39-92)

- `WorkspaceNetworkPolicy` — input policy from above ("we want network /
  we don't")
- `OutboundNatConfig` — exception CIDRs, DNS overrides
- `VmNetworkMode` — resolved decision: `NoEgress` or
  `OutboundNat(plan)`

### State types (lines 101-152)

- `OutboundNatBridgeState` — persisted at run-root level
  (`outbound-bridge-state.json`)
- `OutboundNatVmNetworkState` — persisted per-VM
  (`<run_dir>/network-state.json`)

Both are JSON, both are written atomically.

### Public functions (6 entry points)

| Function | Lines | Role |
|---|---|---|
| `resolve_vm_network_mode()` | 155 | Policy + capability → resolved mode |
| `prepare_vm_network()` | 171 | Bridge/tap/MAC/IP allocation, state persist |
| `inject_guest_network_config()` | 1022 | DNS discovery, runtime rootfs setup |
| `apply_outbound_nat_policy()` | 1053 | iptables NAT/forwarding rules |
| `cleanup_vm_network()` | 1097 | Teardown with bridge owner tracking |
| `read_outbound_nat_vm_network_state()` | 180 | State recovery |

All are called from `lifecycle.rs`.

## Lifecycle of a single OutboundNat VM

### Phase 1: planning (`plan_outbound_nat_vm_network`, lines 224-289)

Deterministic via SHA256 of run-root path:

```
bridge_digest = sha256(run_root_path)
bridge_name = "brfc" + first 12 hex chars of bridge_digest
bridge_octet_2 = bridge_digest[0]            // 16-31 range
bridge_octet_3 = bridge_digest[1]            // 0-255 range
bridge_cidr = 172.<o2>.<o3>.0/24
gateway = 172.<o2>.<o3>.1

vm_digest = sha256(run_root_path + vm_id)
tap_name = "tfc" + first 12 hex chars of vm_digest
guest_mac = "02:" + bytes[0..5] of vm_digest
guest_ipv4 = bridge_cidr.host(2..254 derived from vm_digest[5..7])
```

Why SHA256 of paths? Two reasons:
1. Stable across restarts: same run-root produces same bridge
2. Distinct between concurrent predecessor instances on the same host

**For m80**: this scheme works but can be simpler. m80 has one run-root
per CLI process; deterministic naming based on PID + counter would also
work. But the SHA256 approach is robust against reuse, so keep it.

### Phase 2: collision detection (lines 415-519)

- `reject_guest_ipv4_collision()` (line 415): scans
  `<run_root>/*/network-state.json` for any other VM with this guest IP
- `reject_host_route_collision()` (line 456): reads `/proc/net/route`,
  parses kernel route table, fails if bridge CIDR overlaps any existing
  route

If collision: hard error. No random fallback.

### Phase 3: bridge setup (`ensure_bridge_ready`, lines 291-347)

```bash
# Idempotent: skip if exists and ownership state matches
ip link add <bridge_name> type bridge
ip addr add <gateway>/24 dev <bridge_name>
ip link set dev <bridge_name> up
```

Then writes `outbound-bridge-state.json` at run-root level.

### Phase 4: tap setup (`ensure_tap_ready`, lines 349-413)

```bash
ip tuntap add <tap_name> mode tap
ip link set <tap_name> master <bridge_name>
ip link set <tap_name> up
```

Then writes `network-state.json` at VM run-dir level.

### Phase 5: guest config injection (`inject_guest_network_config`,
lines 1022-1051)

DNS discovery:
1. Try `resolvectl dns` first
2. Fall back to parsing `/etc/resolv.conf`
3. Filter to public-IPv4 only via `is_admitted_dns_resolver()`
   (lines 1467-1489) — rejects private, link-local, loopback, CGN
   (100.64/10), documentation (192.0.2/24, 198.51.100/24,
   203.0.113/24, 240/4), multicast

Then writes systemd config to the per-VM rootfs clone:
- `/etc/systemd/network/10-predecessor-outbound.network` — static IPv4,
  gateway, MAC
- `/etc/systemd/resolved.conf.d/10-predecessor-dns.conf` — admitted DNS

(For m80, rename the unit prefix.)

### Phase 6: NAT policy (`apply_outbound_nat_policy`, lines 1053-1095)

Many iptables operations. From lines 1499 onwards:

```bash
sysctl net.ipv4.ip_forward=1

# Per-VM filter chain
iptables -N tfw<12-hex>

# DNS accept rules (per admitted resolver)
iptables -A tfw<12-hex> -p udp -d <resolver> --dport 53 -j ACCEPT
iptables -A tfw<12-hex> -p tcp -d <resolver> --dport 53 -j ACCEPT

# DNS reject for everything else port 53
iptables -A tfw<12-hex> -p udp --dport 53 -j REJECT
iptables -A tfw<12-hex> -p tcp --dport 53 -j REJECT

# Private exception accept rules
iptables -A tfw<12-hex> -d <exception_cidr> -j ACCEPT

# Permanent-deny list
iptables -A tfw<12-hex> -d <bridge_cidr> -j REJECT
iptables -A tfw<12-hex> -d 169.254.0.0/16 -j REJECT
iptables -A tfw<12-hex> -d 127.0.0.0/8 -j REJECT
# ...

# Default accept
iptables -A tfw<12-hex> -j ACCEPT

# Forwarding entry
iptables -I FORWARD -i <bridge> -s <guest_ip> -j tfw<12-hex>

# NAT masquerade
iptables -t nat -I POSTROUTING -s <guest_ip> -j MASQUERADE
```

All rules are tagged with the per-VM comment prefix
(`LEGACY_RULE_COMMENT_PREFIX`, line 30) so cleanup can find them later
even if the chain naming changes.

## Privilege model

**No privileged-helper binary.** Every privileged operation goes through
`run_privileged_host_command()` at `foundation.rs:860-879`:

```rust
pub(crate) fn run_privileged_host_command(program, args)
    -> Result<(), FirecrackerError>
{
    if !is_effective_root() && has_passwordless_sudo() {
        Command::new("sudo").arg("-n").arg(program).args(args)
    } else {
        Command::new(program).args(args)
    }
}
```

The calling process must be euid 0 OR have passwordless sudo. This is
checked at startup via `verify_jailer_launch_privilege()`
(`foundation.rs:847`).

Commands invoked:
- `ip link add/set` — bridge and tap creation
- `ip tuntap add` — tap device
- `iptables` — filter chains, forwarding, NAT
- `sysctl` — IPv4 forwarding

**For m80**: same model works. Document the requirement clearly. A
future privileged-helper binary is possible but out of scope.

## Address allocation deep dive

### Bridge CIDR

```
sha256(run_root) = b0 b1 b2 ... b31
bridge_octet_2 = (b0 % 16) + 16        // 16..31
bridge_octet_3 = b1                     // 0..255
cidr = 172.<o2>.<o3>.0/24
gateway = 172.<o2>.<o3>.1
```

So bridge CIDRs live in `172.16.0.0/12`. Up to 16*256 = 4096
non-colliding bridges per host.

### Guest IP

```
sha256(run_root + vm_id) = v0 v1 ... v31
guest_ipv4 = bridge_cidr.host_at(2 + (v5 << 8 | v6) % 253)
```

So guest IPs are in `cidr.host(2..254)`, deterministic per `(run_root, vm_id)`.

### Guest MAC

```
guest_mac = "02:" + hex(v0:v1:v2:v3:v4)
```

Locally-administered MAC (the `02:` prefix sets bit 1 of the first
octet, indicating locally-administered).

## Teardown and recovery

`cleanup_vm_network()` (lines 1097-1121):

1. Read state. If missing, call `cleanup_orphan_bridge_if_unused()`
   (line 1106) — scans run-root for other VMs, removes bridge only if
   nothing references it
2. Validate state: run_dir and run_root must match recorded values
3. Remove iptables rules via `cleanup_outbound_nat_policy()`
   (lines 1202-1212):
   - Delete forwarding entries by comment match
   - Delete NAT masquerade rule
   - Delete filter chain rules
   - Delete chain if empty
4. Delete tap device
5. Remove bridge if no sibling VMs reference it
6. Remove state files

### Recovery at startup

`cleanup_orphan_bridge_if_unused()` (line 1123):
- Scans `<run_root>/*/network-state.json` for live VMs
- If none, removes orphan bridge owner record and bridge device
- Tolerates missing/malformed state files; preserves ambiguous or live
  residue (don't delete on uncertainty)

This is the right paranoia level. Crash mid-VM should not break new VMs.

### Robustness

- All teardown ops use `delete_*_if_present()` wrappers (lines 1301,
  1364) — tolerate repeated calls
- Bridge ownership tracked in run-root-level JSON; tap/rule ownership
  via deterministic names + comment prefix
- Crash recovery: next VM startup scavenges orphan bridges via the
  ownership record check

## Coupling to predecessor concepts

### Tightly coupled

| Item | Where |
|---|---|
| `WorkspaceId` / `RunId` in state JSON | `network.rs:115-116` |
| Run-root path drives all naming | `network.rs:243-244` |
| `CapabilityClass::Network` resolves egress | `network.rs:159-161` |
| `VmPaths` and run_dir conventions | `network.rs:174-175` |
| Hardcoded systemd unit names with "predecessor" prefix | `network.rs:28-29` |
| `FirecrackerError` enum | `network.rs:16` |

### Generic / extractable

- Bridge/tap/MAC/IP allocation (deterministic derivation)
- iptables rule construction (no predecessor knowledge)
- `/proc/net/route` parsing (lines 532-568)
- Ipv4Cidr type and overlap detection (lines 2217-2290)

## Refactoring cost for extraction

| Slice | Estimate |
|---|---|
| Extract address allocation | 1-2 days, ~400 lines moved |
| Extract iptables policy generation | 2-3 days, ~600 lines moved |
| Extract bridge/tap I/O with policy abstraction | 3-4 days, ~800 lines moved |
| Replace `FirecrackerError` with m80-local error | 0.5 day |
| Rename "predecessor" prefix in systemd units | 0.5 day |
| Replace `CapabilityClass::Network` with `bool` | 0.5 day |
| Tests | 2-3 days |

**Total: 10-15 days** to fully port `network.rs` with `OutboundNat`
working.

## Recommendation for v0.1

**Skip `OutboundNat` in v0.1.** Ship `NoEgress` only:
- No bridge, no tap, no NAT
- Guest has no network interface beyond loopback
- Saves 2+ weeks of work
- Simplifies privilege model: no iptables needed

`OutboundNat` is the right thing to add in v0.2 once the core lifecycle
is stable.
