# Preflight Capability Review 2026

Behavior capture for `m80-9wm35.7`.

This review walked the current `HostPrerequisiteCheckId::ALL` registry on
2026-05-27. The registry contains 33 stable checks. The older bead text said
27/28 checks; the implementation has grown since then.

## Summary

No code changes are part of this leaf. The review found no second
"both launch paths absent but accepted" class after the systemd/wrapper fix.
Concrete gaps are either already tracked or recorded below:

- `m80-t5ujm.1`: `tap` is a ghost required module and `bridge` lacks a
  built-in/sysfs detection path.
- `m80-t5ujm.3`: `M80_CGROUP_MODE` is parsed at two independent sites.
- `m80-t5ujm.4`: full preflight ordering is still only pinned by privileged
  coverage.
- `m80-73qb3.11`: OutboundNat policy command availability/backend detection.
  This review extended the bead notes so the fix covers `iptables`,
  `iptables-restore`, and `sysctl`.
- `m80-54fs3.5`, `m80-wok08.11`, and `m80-73qb3.2`: hint/message-quality gaps,
  including device-path detail and non-x86 CPU-extension wording.
- `m80-2yvm2.1` and `m80-2yvm2.2`: host memory and run-root capacity policy
  beyond the current minimal run-root free-space check.
- `m80-73qb3.7` and `m80-73qb3.10`: container cgroup delegation and storage
  helper filesystem assumptions.
- `m80-9wm35.8`: documentation refresh for the systemd check and stale module
  wording in operator docs.

## Check Inventory

| Check ID | Capability gated | Review result |
| --- | --- | --- |
| `os_gate` | Host platform is Linux. | Complete for current support policy. |
| `host_kernel_floor` | Linux kernel release parses as 6.1 or newer. | Complete; kernel config inspection is accepted as known-tolerated because concrete requirements are checked through live devices, sysfs, cgroups, and runtime smoke. |
| `kvm` | `/dev/kvm` exists and opens for write. | Complete for KVM access. |
| `cgroup_mode` | Requested cgroup mode is available. | Functional, but parser drift is tracked by `m80-t5ujm.3`; container delegation weakness is tracked by `m80-73qb3.7`. |
| `jailer_identity` | Configured jail UID/GID resolve through host databases. | Complete; preflight intentionally does not create identities. |
| `privilege` | Root or the full required effective capability set. | Complete for startup privilege; mid-launch EPERM hint quality is tracked outside this row by `m80-243wj.11`/`m80-54fs3.5`. |
| `host_substrate_proof` | Distinguishes live preflight from hostless fixture proof. | Complete; not a substitute for real-KVM smoke by design. |
| `kvm_cpu_extensions` | `/proc/cpuinfo` exposes `vmx` or `svm`. | Functional on x86; misleading non-x86 wording is tracked by `m80-73qb3.2`. |
| `kernel_modules` | vsock, TUN/TAP, bridge/netfilter, conntrack, and bridge iptables path. | Drift found and already tracked by `m80-t5ujm.1`; `tap` should not be a required `/proc/modules` token and `bridge` needs a built-in detection path. |
| `ksm_disabled` | KSM side-channel posture. | Complete; explicit skip records operator acceptance. |
| `smt_disabled` | SMT side-channel posture. | Complete as advisory by default with opt-in hard fail. |
| `swap_disabled` | Swap/data-remanence posture. | Complete; explicit skip records operator acceptance. |
| `nested_virt_disabled` | Nested KVM exposure posture. | Complete; explicit skip records operator acceptance. |
| `kvm_timer_floor` | KVM min timer period visibility. | Advisory-only by design; accepted as known-tolerated. |
| `cgroup_favordynmods` | Linux 6.1+ cgroup/KVM mitigation awareness. | Advisory-only by design; accepted as known-tolerated. |
| `transparent_hugepages` | THP host tuning visibility. | Advisory-only by design; no follow-up until hugepage behavior becomes a hard feature gate. |
| `kvm_halt_polling` | KVM halt-poll/timer tuning visibility. | Advisory-only by design. |
| `cpu_governor` | CPU frequency governor visibility. | Advisory-only by design. |
| `cpu_microcode` | CPU0 microcode visibility. | Observational by design; no hard microcode floor currently exists. |
| `cpu_vulnerabilities` | Selected side-channel vulnerability rows. | Complete for current hard-fail set; broad hint/detail improvements are covered by `m80-54fs3.5`/`m80-wok08.11`. |
| `conntrack_capacity` | Host-global conntrack table has capacity for expected VMs. | Complete for the current OutboundNat model. Capacity sizing beyond this bound is tracked by existing performance/security beads. |
| `firecracker_binary` | Firecracker binary path, version, CVE floor, and manifest pairing. | Complete; CVE-floor hint cleanup was absorbed into `m80-54fs3.5`. |
| `firecracker_seccomp_filter` | Firecracker seccomp launch material identity. | Complete; manifest verification opens final paths with `O_NOFOLLOW` and hashes content. |
| `jailer_binary` | Official jailer path and version pairing. | Complete. |
| `systemd` | Host systemd launch availability and selected launch path. | Complete for path selection. Deeper directive-envelope proof is intentionally in systemd launch tests, not preflight, because exercising the full VM/helper envelope mutates host systemd state. |
| `jailer_hardening_wrapper` | Wrapper fallback binary when wrapper path is selected. | Complete after systemd conditional handling. |
| `network_helper` | `m80-net-helper` binary identity. | Complete for the helper binary itself. Policy command availability is covered by `m80-73qb3.11`. |
| `host_binary_manifest` | Installed host binary/material manifest integrity. | Complete for current manifest contract. |
| `kernel_image` | Kernel artifact discovery/override. | Complete; kernel config gaps remain operator documentation unless a concrete runtime dependency lacks a live gate. |
| `rootfs_manifest` | Rootfs, manifest, build receipt, provenance, and rootfs fd pinning. | Complete for artifact integrity. Bit-flip after preflight is separately tracked by `m80-uh6ex.6`. |
| `run_root` | Run-root absolute path, existence, device-node-capable mount, and minimum free space. | Basic gate complete; stronger capacity policy is tracked by `m80-2yvm2.2`. |
| `run_root_filesystem` | Reflink support advisory for overlay clone mode. | Advisory-only by design; filesystem-specific fallocate assumptions are tracked by `m80-73qb3.10`. |
| `storage_helpers` | `mkfs.ext4`, `cp`, `fallocate`, `debugfs`, `e2fsck` on `PATH`. | Complete for storage/image operations. |

## Accepted Known-Tolerated Gaps

- No `/proc/config.gz` sweep. m80 checks concrete live capabilities instead of
  kernel build flags. The known exception, bridge built-in detection, is filed
  as `m80-t5ujm.1`.
- No preflight probe for `/proc/sys/kernel/unprivileged_userns_clone` or
  `/proc/sys/user/max_user_namespaces`. Current m80 launch paths are root/cap
  bearing and do not depend on unprivileged user namespaces.
- No rootless or user-mode systemd support. ADR 0010 deliberately chooses
  system systemd transient units or the wrapper fallback.
- No full systemd directive-envelope preflight. The no-op transient-unit probe
  is enough to select the path; the actual Firecracker and network-helper
  directive envelopes are pinned by launch/unit tests and real-KVM smoke.

## Documentation Drift To Fix In `m80-9wm35.8`

The `m80-preflight` README and host setup docs still contain wording shaped by
the older check count and old module assumptions. `m80-9wm35.8` should update
that public contract after this audit so the `systemd` row,
`firecracker_seccomp_filter` row, and stale `tap` wording do not drift from the
current registry.
