# 04 — Infra and artifacts: kernel, rootfs, jailer, manifests, preflight

## Files of interest

- `/tank/projects/predecessor/infra/firecracker/prepare-guestd-image.sh`
- `/tank/projects/predecessor/scripts/firecracker-preflight.sh`
- `/tank/projects/predecessor/scripts/firecracker-dev-env.sh`
- `/tank/projects/predecessor/infra/firecracker/lima/predecessor-fc-dev.yaml`
- `/tank/projects/predecessor/infra/firecracker/provision-guest.sh`

## End-to-end "make a host firecracker-ready" flow

1. Install Linux/KVM host or Lima guest (macOS dev only)
2. Run `provision-guest.sh` → downloads firecracker, jailer, kernel,
   rootfs to `/opt/firecracker/`
3. Run `prepare-guestd-image.sh` → builds `guestd-rs`, customizes rootfs,
   writes manifest
4. Run `firecracker-preflight.sh` → verifies host is ready
5. Spawn VMs via the library

m80 inherits this exact shape but renames everything and slims the
defaults.

## Kernel

**Source**: downloaded from the AWS S3 firecracker-ci bucket.

`provision-guest.sh:62-64`:

```bash
KERNEL_BASE="https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/${FC_CI_VERSION}/${ARCH}"
# Latest vmlinux-* in that bucket, sorted by version
```

Version: **Linux 6.1.155** (or whatever firecracker-ci ships for the
chosen Firecracker version).

Path: `/opt/firecracker/artifacts/vmlinux-6.1.155`.

Config: not in-tree. The kernel is a managed immutable artifact pinned
to the Firecracker release cycle.

**For m80**: keep this exact pattern. The firecracker-ci bucket is
publicly readable and well-versioned. Vendor the URL, pin a version, let
users override via env.

## Rootfs

**Build script**: `infra/firecracker/prepare-guestd-image.sh` (~326 lines).

### Pipeline

1. **Source rootfs**: Ubuntu LTS `.squashfs` from the firecracker-ci
   bucket → `.ext4` (resized to 1 GiB by `truncate -s 1G`)
   (`provision-guest.sh:80-89`)
2. **Mount via loop device** (`prepare-guestd-image.sh:201`)
3. **Chroot into guest**, install python3 and nodejs via apt
   (`prepare_guest_package_chroot`, lines 122-152)
4. **Install binaries and units**:
   - `/usr/local/bin/guestd-rs` (built from `cargo build -p guestd-rs`,
     lines 68-73)
   - `/etc/systemd/system/guestd-rs.service`
   - `/etc/systemd/system/var-lib-predecessor-workspace.mount`
   - `/etc/default/guestd-rs` (env file)
   - `/var/lib/predecessor/workspace` directory
5. **Enable units** in `multi-user.target.wants/` (lines 220-221)
6. **Generate manifest** beside the rootfs (lines 244-314)
7. **Unmount, sha256 the result**

### Output

`guestd-control.ext4` typically ~1 GiB after expansion.

### Strict policy

The script *fails the build* if `npm`, `pip`, or `pip3` are detected in
the resulting image (lines 183-194). This is a predecessor "no
package-manager surfaces" policy. **m80 should drop this strictness** —
let users build images with whatever they want.

## Provenance manifest

Filename: `<rootfs-path>.manifest.json` next to the ext4.

Schema version: `1` (constant in script:
`FIRECRACKER_GUESTD_MANIFEST_SCHEMA_VERSION`).

### Fields

```json
{
  "schema_version": 1,
  "expected_firecracker_version": "v1.15.1",
  "kernel_image": "/opt/firecracker/artifacts/vmlinux-6.1.155",
  "kernel_image_sha256": "<hex>",
  "kernel_image_size_bytes": <int>,
  "source_rootfs_image": "<path to source squashfs/ext4>",
  "source_rootfs_sha256": "<hex>",
  "source_rootfs_size_bytes": <int>,
  "output_rootfs_image": "/opt/firecracker/artifacts/guestd-control.ext4",
  "output_rootfs_sha256": "<hex>",
  "output_rootfs_size_bytes": <int>,
  "guest_image_id": "guestd-control",
  "guestd_binary": "<build path>",
  "guestd_binary_path": "/usr/local/bin/guestd-rs",
  "guestd_binary_sha256": "<hex>",
  "guestd_service_unit": "guestd-rs.service",
  "guestd_service_sha256": "<hex>",
  "workspace_mount_unit": "var-lib-predecessor-workspace.mount",
  "workspace_mount_sha256": "<hex>",
  "workspace_mount_path": "/var/lib/predecessor/workspace",
  "guest_runtime_packages": ["python3", "nodejs"],
  "guest_runtime": {
    "inventory_source": "managed-rootfs-chroot",
    "os_release": { "id": "ubuntu", "version_id": "...", "pretty_name": "..." },
    "required_interpreters": [
      { "name": "python3", "package": "python3", "path": "/usr/bin/python3",
        "version_command": "...", "version_output": "..." },
      { "name": "node", "package": "nodejs", "path": "/usr/bin/node",
        "version_command": "...", "version_output": "..." }
    ],
    "package_managers": [
      { "name": "npm",  "required": false, "present": false },
      { "name": "pip",  "required": false, "present": false },
      { "name": "pip3", "required": false, "present": false }
    ]
  },
  "boot_target": "multi-user.target",
  "guest_port": 9001,
  "ready_marker": "GUESTD_READY",
  "no_egress_reason": "no-egress: ...",
  "guestd_build_profile": "debug|release",
  "guestd_failure_mode": ""
}
```

### Why this is good

- Reproducibility: every byte in the boot identity is hashed
- Tampering: preflight refuses to boot if any hash is stale
- Inventory: caller knows what interpreters are available without
  shelling into the image

### m80 manifest changes

- Drop `guest_runtime_packages` constraint (let images carry whatever)
- Drop `package_managers` constraint
- Keep boot_target / guest_port / ready_marker as configurable
- Keep all sha256 fields — they're load-bearing for safety

## Host preflight

`scripts/firecracker-preflight.sh` runs ten checks before declaring a
host ready (lines 195-249):

1. **OS check**: `uname -s` must be Linux
2. **KVM access**: `/dev/kvm` exists and is writable (or sudo-writable
   for jailer mode)
3. **Firecracker binary**: `FIRECRACKER_BIN` or `/opt/firecracker/bin/firecracker`
   exists and is executable
4. **Jailer binary**: same for `jailer`
5. **Version check**: both binaries respond to `--version`; if
   `FIRECRACKER_VERSION` set, must match
6. **Kernel artifact**: auto-discover `find /opt/firecracker/artifacts
   -name 'vmlinux-*'`, sort by version, must be absolute
7. **Rootfs artifact**: env override, fall back to default,
   must be absolute
8. **Manifest**: `<rootfs-path>.manifest.json` must exist; passes a
   Python schema validator (lines 72-174)
9. **Run root**: `FIRECRACKER_RUN_ROOT` or `/tmp/predecessor-firecracker-run`
   must be absolute, creatable, writable
10. **Storage helpers**: `mkfs.ext4`, `debugfs`, `e2fsck` must be on PATH

All checks are fail-closed; any missing resource exits with explicit error.

**For m80**: keep this exact structure. Rename env vars (`M80_BIN`,
`M80_RUN_ROOT`, etc.). Make the binary names configurable so users with
existing firecracker installs don't need to duplicate.

## Dev environment script

`scripts/firecracker-dev-env.sh` wraps Lima + provisioning + smoke
tests. Subcommands:

| Subcommand | Action |
|---|---|
| `create` | Provision Lima guest from `infra/firecracker/lima/predecessor-fc-dev.yaml` |
| `start` / `stop` / `delete` / `shell` | Vanilla limactl |
| `exec <cmd>` | Run command in guest with cargo env |
| `provision` | Copy + run `provision-guest.sh` inside guest |
| `prepare-guestd-image` | Build guestd, run `prepare-guestd-image.sh` |
| `smoke` | UDS API smoke test |
| `rust-smoke` | `cargo test -p agent-sandbox-firecracker --test minimal_boot` |
| `control-smoke` | vsock guest control test |
| `backend-smoke` | `SandboxBackend` conformance |
| `predecessor-e2e` | full end-to-end via `agent-tool-executor` |

Defaults:
- Firecracker version: `v1.15.1`
- Kernel: auto-discover `/opt/firecracker/artifacts/vmlinux-*`
- Rootfs: `/opt/firecracker/artifacts/guestd-control.ext4`
- Run root: `/tmp/predecessor-firecracker-run`

**For m80**: this entire script is mostly portable. Rename, slim defaults.

## Lima for macOS

**Required for macOS dev only.** Linux dev skips Lima entirely.

`infra/firecracker/lima/predecessor-fc-dev.yaml`:
- Base: Ubuntu LTS
- VM type: `vz` (Apple Virtualization framework)
- Arch: aarch64
- Resources: 4 CPU, 8 GiB RAM, 60 GiB disk
- **Nested virtualization: enabled** — critical for `/dev/kvm`
  passthrough
- Probe: asserts `/dev/kvm` exists inside guest; setup fails if not

The README on the firecracker crate explicitly notes (lines 7-8): "If
the Lima guest does not expose writable `/dev/kvm`, stop local probing
and switch to the same Linux-side setup on a remote Linux/KVM host
instead."

**For m80**: keep the Lima config as a dev convenience for macOS users,
but be honest in docs that this requires Apple Silicon + recent macOS for
nested virt support.

## Firecracker and jailer binaries

Both expected pre-installed; not built in repo.

- **Source**: GitHub releases at `firecracker-microvm/firecracker`
  (`provision-guest.sh:59`)
- **Default version**: `v1.15.1`
- **Default install path**: `/opt/firecracker/bin/{firecracker,jailer}`
- **Override via env**: `FIRECRACKER_BIN`, `JAILER_BIN`,
  `FIRECRACKER_VERSION`

## Networking host requirements

Currently first-line is **NoEgress only** for the v1 contract. Required:
nothing beyond KVM and helper binaries.

For the future `OutboundNat` mode (frozen contract, partially
implemented):
- IPv4 forwarding via `sysctl net.ipv4.ip_forward=1`
- `iptables` (or nftables, but the code uses iptables)
- `iproute2` (`ip` command for bridge/tap)
- `bridge` and `tap` kernel modules (modern kernels include both)
- `resolvectl` or `/etc/resolv.conf` for host DNS discovery

These are documented in
`docs/gates/stage-g-firecracker-outbound-network-contract.md`.

**For m80**: ship `NoEgress` in v0.1, add `OutboundNat` in v0.2. The
implementation is largely portable but requires effective root or
passwordless sudo on the host (no privileged-helper binary).

## Hand-rolled vs. off-the-shelf

| Component | Source | Status |
|---|---|---|
| Firecracker binary | GitHub releases | Off-the-shelf |
| Jailer binary | GitHub releases | Off-the-shelf |
| Kernel | firecracker-ci S3 bucket | Off-the-shelf |
| Base rootfs | firecracker-ci S3 bucket | Off-the-shelf |
| `guestd-rs` daemon | predecessor in-tree | Hand-rolled (rewrite for m80) |
| `prepare-guestd-image.sh` | predecessor in-tree | Hand-rolled (port to m80) |
| Manifest schema | predecessor in-tree | Hand-rolled (port + slim) |
| `firecracker-preflight.sh` | predecessor in-tree | Hand-rolled (port + rename) |
| Lima dev YAML | predecessor in-tree | Hand-rolled (port) |
| Per-VM rootfs cloning | `storage.rs` | Hand-rolled (port) |
| Vsock guest control protocol | `agent-guest-proto` | Hand-rolled (rewrite slim) |
| NAT/iptables for OutboundNat | `network.rs` | Hand-rolled (port carefully) |
| Boot identity recording | `boot.rs` | Hand-rolled (port) |
| Writeback authority hooks | `backend.rs` | Hand-rolled (rethink for CLI) |
| Failure-triage archiving | `diagnostics.rs` | Hand-rolled (defer) |

Net: m80 ships ~7-8 hand-rolled components, all portable from predecessor with
mostly-mechanical changes.
