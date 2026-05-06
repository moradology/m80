# Outbound NAT Guest Network Injection

## Networkd Unit

The guest network injection phase writes
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

The guest network injection phase writes
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

## Image Neutrality

The guest daemon does not write `/etc/systemd/network`, `/etc/resolv.conf`,
or interface state at runtime. m80 performs this phase from the host before
boot by writing the supplied runtime ext4 image; the guest then relies on the
image's normal systemd-networkd and systemd-resolved startup behavior to apply
the configuration.

This helper is intentionally parameterized by a runtime rootfs image path.
After the storage pivot, callers must pass the per-VM writable rootfs image
that actually contributes files to the guest's merged root, rather than
mutating the shared read-only base.

Source: predecessor host-side injection path lines 1022-1051 and the Stage-G
image-neutrality contract.

Verification:
`crates/m80-net-outbound/tests/network-outbound-nat/injection.rs::guest_daemon_does_not_touch_networking`.
