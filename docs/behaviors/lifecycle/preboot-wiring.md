# Preboot Wiring

## Machine Config

Before `InstanceStart`, m80 first PUTs `/machine-config` with the sandbox's
vCPU count, memory size, and `smt=false`. Omitted sizing uses the m80 defaults:
1 vCPU and 1024 MiB. This is the m80 form of the predecessor behavior recorded at
`crates/sandbox/agent-sandbox-firecracker/src/boot.rs:374` and
`crates/sandbox/agent-sandbox-firecracker/src/client.rs:134`.

`m80-firecracker` builds this as the first entry in its preboot PUT plan, and
`phase_11_rest_puts` applies that plan before the launch path issues
`InstanceStart`.

## Boot Source

m80 PUTs `/boot-source` after machine config and before any drives. The kernel
is bind-mounted into the jailer chroot at `/kernel`, so Firecracker receives
`kernel_image_path=/kernel`. Boot args are selected from the image kind and
kernel kind, unless `SandboxConfig::boot_args` overrides the entire command
line. This captures the m80 equivalent of predecessor's `BootSourceConfig`
construction at `crates/sandbox/agent-sandbox-firecracker/src/boot.rs:115-122`
and REST PUT at `client.rs:138`.

## Root Drive

m80 PUTs two root-filesystem-related drives in order:

1. `rootfs`: `/rootfs.ext4`, root device, read-only.
2. `rootfs_overlay`: `/rootfs.overlay.ext4`, non-root, read-write.

The first drive is the shared immutable base ext4 and becomes `/dev/vda`. The
second is the per-VM sparse overlay and becomes `/dev/vdb`; guestd uses it as
the overlayfs upperdir. This deliberately differs from the older predecessor
runtime-rootfs clone described around
`crates/sandbox/agent-sandbox-firecracker/src/boot.rs:39-77`: m80 keeps the
base rootfs shared and writable state in a per-VM overlay.

## Scratch Drive

When `SandboxConfig::workspace` is present, m80 PUTs a third drive before the
vsock device:

- `workspace`: `/scratch.ext4`, non-root, read-write.

The scratch image becomes `/dev/vdc` and is mounted by guestd at `/workspace`.
When no workspace is configured, this drive is omitted. This is the m80 form of
the predecessor workspace drive behavior recorded at
`crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:665-670`, with the
current m80 filename `/scratch.ext4` rather than predecessor's older
`workspace.ext4` wording.

## Vsock Device

m80 PUTs `/vsock` after the drive PUTs and before `InstanceStart`. The guest
CID is derived from `vm_id`, and the UDS path is `/vsock.sock` inside the
jailer chroot. This captures the m80 equivalent of predecessor's guestd-vsock
config at `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:658-661`
and REST PUT at `client.rs:147`.

## Boot Identity

After preboot REST wiring succeeds and before `InstanceStart`, m80 writes
`<run_dir>/boot-identity.json`. The record ties the admitted kernel path,
rootfs path, kernel kind, image kind, kernel sha256, rootfs sha256, manifest
sha256, expected Firecracker version, guest port, ready marker, and boot target
to the run directory.

The input data is already validated by `m80-preflight`; `m80-firecracker`
records the admitted identity rather than recomputing artifact sha256s in the
launch path. This is the m80 form of the older predecessor behavior recorded at
`crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:613,719`.
