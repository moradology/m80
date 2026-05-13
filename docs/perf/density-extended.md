# Extended Density Sweep

Bead: `m80-jp6ik.42`.

Raw artifacts:

- `crates/m80-firecracker/benches/density-extended.csv`
- `crates/m80-firecracker/benches/snapshots/density-extended.json`

## Method

Run date: 2026-05-13.

Command:

```sh
N=20 WARMUP=2 KIND=minimal KERNEL_KIND=stripped \
  IMAGE_BUILD_DIR_MINIMAL=/tmp/m80-build/minimal-jp6ik42 \
  M80_BIN=./target/release/m80 \
  bash scripts/bench-density-extended.sh
```

Host: Linux 6.17.0-22-generic x86_64, 48 logical CPUs,
`acpi-cpufreq` with `schedutil`.

Runtime: `m80 0.0.0`, Firecracker v1.15.1, real `/dev/kvm`, real jailer,
sudo cleanup between attempts, no explicit page-cache drop.

Image bundle:

- root: `/tmp/m80-build/minimal-jp6ik42`
- rootfs: `output.ext4`, 268435456 bytes,
  sha256 `420e8f0797b8c2a8ae1e8e5c89fb5ce1d515cd5148929bbb693d6f7346c6710b`
- kernel: `vmlinux`, 17966848 bytes,
  sha256 `5df58bad49a4b425f51ec56d3708b7230e29bc1cc7ba2f42c1b632a97d7a53d2`

## Results

| egress | C | attempts | successful VMs | failures | wall P50 | wall P95 | wall P99 | per-VM P99 | per-VM max |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| none | 1 | 20 | 20 | 0 | 1516 ms | 1517 ms | 1518 ms | 1518 ms | 1518 ms |
| none | 2 | 20 | 40 | 0 | 1517 ms | 1518 ms | 1518 ms | 1518 ms | 1518 ms |
| none | 4 | 20 | 79 | 1 | 1517 ms | 1519 ms | 1519 ms | 1519 ms | 1519 ms |
| none | 8 | 20 | 160 | 0 | 1517 ms | 1518 ms | 1518 ms | 1518 ms | 1518 ms |
| none | 16 | 20 | 320 | 0 | 1517 ms | 1615 ms | 1616 ms | 1615 ms | 1616 ms |
| none | 32 | 20 | 640 | 0 | 1715 ms | 1718 ms | 1719 ms | 1718 ms | 1719 ms |
| none | 48 | 20 | 957 | 3 | 1819 ms | 1916 ms | 1917 ms | 1823 ms | 1917 ms |
| none | 64 | 20 | 1278 | 2 | 1932 ms | 2028 ms | 2032 ms | 1946 ms | 2032 ms |
| outbound | 1 | 20 | 20 | 0 | 2316 ms | 2318 ms | 2319 ms | 2319 ms | 2319 ms |
| outbound | 2 | 20 | 40 | 0 | 2817 ms | 2917 ms | 2917 ms | 2917 ms | 2917 ms |
| outbound | 4 | 20 | 80 | 0 | 3618 ms | 3718 ms | 3718 ms | 3718 ms | 3718 ms |
| outbound | 8 | 20 | 158 | 2 | 5118 ms | 5219 ms | 5317 ms | 5219 ms | 5317 ms |
| outbound | 16 | 20 | 308 | 12 | 8120 ms | 8421 ms | 8520 ms | 8420 ms | 8520 ms |
| outbound | 32 | 20 | 593 | 47 | 13925 ms | 14728 ms | 14926 ms | 14725 ms | 14926 ms |

## Interpretation

No-egress stays flat through C=16 and first shows a meaningful wall at C=32.
Wall-time-to-all-ready P50 rises from 1517 ms at C=16 to 1715 ms at C=32, then
1932 ms at C=64. The high-C no-egress failure count is low but nonzero, so C=48
and C=64 are useful saturation probes rather than clean production targets.

Outbound has a wall immediately above the single-VM case. P50 rises from
2316 ms at C=1 to 3618 ms at C=4, 5118 ms at C=8, 8120 ms at C=16, and
13925 ms at C=32. The C=16 and C=32 failure counts are also high enough that
the first product-significant outbound wall is C=8, while C=16/C=32 are
failure-shape evidence for the next density work.

The phase snapshots taken during the outbound cells point at host networking,
not guest userspace, as the density wall. At C=32 outbound, the per-cell P50
showed `phase_6_network_realize` around 456 ms and
`phase_7_outbound_guest_config` around 142 ms per VM, while the guest
network-configured milestone stayed around 46 ms elapsed. The run also exposed
two measurement-hygiene defects that are now fixed in the same diff:

- The stripped kernel seed now keeps `CONFIG_NETDEVICES=y`, so `olddefconfig`
  preserves built-in `CONFIG_VIRTIO_NET=y` and outbound guests see `eth0`.
- The outbound network allocation lock now covers bridge creation/recovery as
  well as VM IP/state allocation, preventing concurrent processes from racing
  the same run-root bridge.

This feeds `m80-jp6ik.13`: no-egress host launch does not become the first
wall until C=32, while outbound density already needs network critical-section
work by C=8. It also feeds `m80-jp6ik.20`: outbound C=16/C=32 still has enough
wall time and failures to justify replacing serialized iptables work with an
atomic backend if `.13` does not remove the bottleneck.
