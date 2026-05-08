# Outbound NAT Guest Network Injection

## PID-1 Cmdline Injection

Current Ubuntu and Minimal m80 images boot `m80-guestd` as PID 1 with
`init=/m80-guestd`. They do not rely on systemd-networkd or systemd-resolved
for outbound setup.

For these images, the host prepares networking by appending bounded
`m80.net.*` tokens to the kernel command line:

- `m80.net=outbound`
- `m80.net.iface=eth0`
- `m80.net.mac=<guest_mac>`
- `m80.net.ipv4=<guest_ipv4>/<prefix>`
- `m80.net.gateway=<bridge_gateway>`
- `m80.net.dns=<resolver>[,<resolver>...]`

`m80-guestd` parses those tokens after the overlay pivot and before it emits
ready. It configures the guest interface directly through rtnetlink and writes
`/etc/resolv.conf` in the pivoted root. It does not shell out to `ip`,
`ifconfig`, `resolvectl`, systemd-networkd, or systemd-resolved.

The host-side current-image entrypoint is
`prepare_pid_one_network_cmdline(...)`: it discovers admitted public IPv4 DNS
resolvers, records them in `<run_dir>/network-state.json`, marks the guest
network config phase complete, and returns deterministic cmdline tokens for
the launcher to append.

`m80-firecracker` runs that entrypoint during launch phase 7 after bridge/TAP
realization and before Firecracker `InstanceStart`. The tokens are appended to
the boot-source args, and phase 11 emits the matching `eth0` network-interface
PUT with the realized TAP name and guest MAC.

Verification:
`crates/m80-guestd/src/pid_one_network.rs::tests::*` and
`crates/m80-net-outbound/tests/network-outbound-nat/injection.rs::pid_one_*`.
The launch/preboot composition is pinned by
`crates/m80-firecracker/src/preboot.rs::tests::boot_args_append_pid_one_network_tokens_after_workspace_marker`
and
`crates/m80-firecracker/src/preboot.rs::tests::outbound_nat_network_interface_put_after_drives_and_before_vsock`.

## Networkd Unit

The systemd-image guest network injection phase writes
`/etc/systemd/network/10-m80-outbound.network` into the supplied per-VM
runtime rootfs ext4 image with `debugfs`. The unit contains:

- `[Match] MACAddress=<guest_mac>`
- `[Network] Address=<guest_ipv4>/<prefix>`
- `Gateway=<bridge_gateway>`
- one `DNS=<resolver>` line per admitted resolver
- `IPv6AcceptRA=no`
- `LinkLocalAddressing=no`

The helper creates `/etc/systemd/network` in the image when `debugfs stat`
does not find it.

Source: predecessor `build_guest_network_config` lines 1381-1411 and
`write_guest_network_config` lines 2020-2029, with unit path renamed from
`10-predecessor-outbound.network` to `10-m80-outbound.network`.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/injection.rs::networkd_unit_injected_with_static_ip_and_dns`.

## Resolved Dropin

The systemd-image guest network injection phase writes
`/etc/systemd/resolved.conf.d/10-m80-dns.conf` into the supplied per-VM
runtime rootfs ext4 image with `debugfs`. The drop-in contains `[Resolve]`,
`DNS=<space-separated admitted resolvers>`, an empty `FallbackDNS=`, and
`Domains=~.` so all guest DNS routes through the admitted upstream set.

The helper creates `/etc/systemd/resolved.conf.d` in the image when
`debugfs stat` does not find it.

Source: predecessor `build_guest_network_config` line 1409 and
`write_guest_network_config` lines 2020-2029, with drop-in path renamed from
`10-predecessor-dns.conf` to `10-m80-dns.conf`.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/injection.rs::resolved_dropin_injected_with_admitted_dns`.

## Systemd Image Path

The debugfs-based networkd/resolved writer is retained for a future image mode
that boots systemd as init. It is not the current Ubuntu/Minimal PID-1 path.

This helper is intentionally parameterized by a runtime rootfs image path.
After the storage pivot, callers must pass the per-VM writable rootfs image
that actually contributes files to the guest's merged root, rather than
mutating the shared read-only base.

Source: predecessor host-side injection path lines 1022-1051 and the Stage-G
image-neutrality contract.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/injection.rs::networkd_unit_injected_with_static_ip_and_dns`.
