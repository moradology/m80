# Outbound NAT Address Allocation

Behavior capture for beads `m80-exy.1.1` through `m80-exy.1.6`.

## Bridge Name

The system computes the bridge interface name as `brfc` plus the first 11
hexadecimal characters of `sha256(run_root_path)`.

This yields a deterministic interface name for each run-root and keeps the
Linux interface name within the 15-byte limit. This behavior comes from the
predecessor outbound network planner in
`crates/sandbox/agent-sandbox-firecracker/src/network.rs`, Phase 1 planning,
around lines 224-289. The skeleton bead originally said 12 hex characters, but
the predecessor source uses 11 at line 245; `brfc` plus 12 hex characters would be
16 visible bytes and would exceed Linux's 15-byte interface-name limit.

## Bridge CIDR

The system derives the bridge CIDR from the run-root digest as:

```text
172.<o2>.<o3>.0/24
o2 = (sha256(run_root_path)[0] % 16) + 16
o3 = sha256(run_root_path)[1]
```

The gateway address is the `.1` address in the derived `/24`. This mirrors the
predecessor address allocation formula around lines 246-249 of the same network
planner.

## Bridge CIDR Cap

All derived bridge CIDRs live inside `172.16.0.0/12` because the second octet is
constrained to `16..=31`. The third octet spans `0..=255`, so one host has
`16 * 256 = 4096` deterministic bridge CIDRs before run-root digest collisions
are possible.

## Tap Name

The system computes each VM tap interface name as `tfc` plus the first 12
hexadecimal characters of `sha256(run_root_path || vm_id)`.

The name is deterministic for the `(run_root, vm_id)` pair and stays within the
15-byte Linux interface name limit. This matches the predecessor Phase 1 tap
allocation behavior around lines 251-253.

## Guest IPv4

The system derives the guest IPv4 from the VM digest and the bridge CIDR:

```text
host = 2 + (u16_be(vm_digest[5], vm_digest[6]) % 253)
guest_ipv4 = bridge_cidr.host(host)
```

The host octet is always in `2..=254`, leaving `.0` for the network address,
`.1` for the gateway, and `.255` for broadcast.

## Guest MAC

The system formats the guest MAC as a locally administered unicast address:

```text
02:<vm_digest[0]>:<vm_digest[1]>:<vm_digest[2]>:<vm_digest[3]>:<vm_digest[4]>
```

The first octet `0x02` sets the locally-administered bit and clears the
multicast bit. The remaining five octets come from the same VM digest used for
the tap name and guest IPv4. This captures the predecessor guest MAC behavior
around lines 258-265 and the validation constraint around lines 761-785.

## Verification

`crates/m80-net-outbound/tests/network-outbound-nat/address-allocation.rs`
pins the bridge name, bridge CIDR, `172.16.0.0/12` cap, tap name, guest IPv4,
and guest MAC formulas with deterministic fixture values.
