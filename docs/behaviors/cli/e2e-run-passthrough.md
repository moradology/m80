# CLI Run Passthrough E2E

Behavior capture for bead `m80-lt15.5`.

## Contract

`m80 run` is a process facade, not a VM-control command:

1. Launch one sandbox from the selected profile.
2. Run the requested guest process.
3. Copy guest stdout bytes to host stdout.
4. Copy guest stderr bytes to host stderr.
5. Exit with the guest process exit code.
6. Stop and delete the sandbox state before returning.

Wrapper diagnostics must not appear on stdout in pipe mode. If m80 needs to
report wrapper failures, warnings, or retained diagnostics, they go to stderr.

## Ignored KVM Test

`crates/m80-cli/tests/e2e_run_passthrough.rs` spawns the `m80` binary with a
real Firecracker/KVM backend. It is ignored by default because it needs:

- `M80_FIRECRACKER_BIN`
- `M80_JAILER_BIN`
- `M80_KERNEL_IMAGE`
- `M80_ROOTFS_IMAGE`
- a Linux host with writable `/dev/kvm`
- enough privilege for Firecracker/jailer setup

The fixture sets its own `HOME`, `M80_RUN_ROOT`, `M80_CGROUP_MODE=disabled`,
and `M80_MAX_CONCURRENT_VMS=1` so user config and shared run-root state do not
change the assertions.

## Scenarios

The passing case mounts a workspace, sets `--cwd /workspace`, reads a marker
file from the guest, writes guest stderr, and exits 0. This proves requested
workspace visibility and separated stdout/stderr in the same user-facing run.

The failing case writes both streams and exits 17. The `m80` process must exit
17 while still stopping and deleting the sandbox. Nonzero guest exits are not
wrapper failures.

Both scenarios dump command status, stdout, stderr, and any remaining run-root
diagnostics on assertion failure so host/guest debugging starts with evidence.
