# Privileged Nested-KVM Runner

This runbook provisions a clean L1 VM for m80 release and real-KVM proof work.
The shape is:

```text
L0 bare-metal AMD/Intel KVM host -> L1 qemu/libvirt VM -> L2 Firecracker VM
```

Firecracker cannot expose nested KVM to its guest, so the L1 runner is a normal
qemu/libvirt VM with `host-passthrough` CPU. m80 then runs Firecracker inside
that L1.

The L0 host must expose VMX/SVM into the L1. The L1's own KVM module must not
expose nested virtualization onward to Firecracker guests:
`/sys/module/kvm_amd/parameters/nested` or
`/sys/module/kvm_intel/parameters/nested` should read `0`, `N`, or `n` inside
the L1.

## L0 Host Setup

Run this once on the bare-metal host:

```sh
scripts/setup-privileged-runner.sh
```

The setup is idempotent. It installs qemu/libvirt/cloud-init tooling, loads the
KVM, TUN, bridge, and vsock modules, starts libvirt, activates the default NAT
network, disables KSM for the current boot, and verifies `/dev/kvm`,
`/dev/net/tun`, and nested KVM mode.

Host requirements:

- KVM-capable x86_64 CPU with VMX or SVM exposed.
- Linux kernel with `kvm`, `kvm_intel` or `kvm_amd`, `tun`, `bridge`, and
  `vhost_vsock` modules available.
- Nested KVM enabled: `/sys/module/kvm_intel/parameters/nested` or
  `/sys/module/kvm_amd/parameters/nested` reads `Y`, `y`, or `1`.
- `/dev/kvm` readable/writable by the runner account through the `kvm` group or
  `sudo`; `/dev/net/tun` present.
- KSM disabled: `/sys/kernel/mm/ksm/run` reads `0` when that path exists. Run
  `scripts/setup-privileged-runner.sh` after reboot if the host re-enables it.
- Passwordless `sudo -n` for host setup, iptables/netlink cleanup, and
  privileged E2E commands when the runner is not root.

The setup script installs this Ubuntu package set when `--no-apt` is not used:
`qemu-system-x86`, `qemu-utils`, `libvirt-daemon-system`, `libvirt-clients`,
`virtinst`, `cloud-image-utils`, `dnsmasq-base`, `bridge-utils`, `iproute2`,
`iptables`, `ipset`, `linux-headers-generic`, and `build-essential`.

Each spawned L1 installs the runtime/build tools used by the privileged
workflow: stable Rust via rustup, `build-essential`, `busybox-static`,
`ripgrep`, and `squashfs-tools`.

For a read-only check:

```sh
scripts/setup-privileged-runner.sh --dry-run
```

For machine-readable state:

```sh
scripts/setup-privileged-runner.sh --json
```

## Spawn An L1

Create the runner:

```sh
scripts/spawn-l1-runner.sh create
```

Defaults:

- state/cache root: `/tank/tmp/m80-l1-runner`
- domain name: `m80-l1-runner`
- image: Ubuntu Noble cloud image
- CPU: `host-passthrough`
- Firecracker train: `v1.15.1`
- jail identity: UID/GID `3000` (`m80jail`)

The command waits for cloud-init, SSH, `/dev/kvm`, the nested CPU flag, the
default jail identity, the L1 KVM module with nested mode disabled, the
official Firecracker binary, the official jailer binary, and the compiled
Firecracker seccomp filter. On success it prints:

```sh
M80_L1_NAME=...
M80_L1_IP=...
M80_L1_USER=...
M80_L1_SSH_KEY=...
M80_L1_SSH_TARGET=...
M80_L1_SSH_OPTS=...
```

Run a command inside the L1:

```sh
scripts/spawn-l1-runner.sh ssh -- 'test -r /dev/kvm && grep -qw svm /proc/cpuinfo'
```

Destroy the L1 and its local state:

```sh
scripts/spawn-l1-runner.sh destroy
```

## Validate A Public Release

Local isolation levels A-C:

```sh
scripts/validate-release.sh --level a-c --tag latest
```

That command resolves public latest, downloads the release assets, verifies the
release integrity material, verifies the bundle, verifies the install handoff,
runs the bounded public freshness verifier, and installs into a temporary
install root. The level-C lane tries the no-sudo install-root path first. If the
published installer rejects it at the live preflight privilege gate, the harness
reruns the same temporary install-root fixture with `sudo -n` and records that
fallback in `level-c.json`; it still does not write the default `/opt/m80`
install root.

Nested-KVM release smoke:

```sh
scripts/validate-release.sh \
  --level e \
  --tag latest \
  --spawn-l1 \
  --proof-out docs/proofs/release/<tag>-nested-kvm-release-smoke.json
```

The level-E proof runs inside the L1:

1. verify `/dev/kvm` is readable and writable;
2. verify the nested CPU flag is present;
3. download the public release `install.sh`;
4. install the release into a temporary install root;
5. run `m80 run -- echo hello` through the documented `M80_DEFAULT_PROFILE=env`
   fixture profile pointing at the temporary install root;
6. capture stdout, stderr, exit status, Firecracker/jailer versions, installed
   status JSON, and cleanup evidence;
7. remove the temporary install root.

Local harness state stays under the host work root. Remote level-E scratch state
defaults to `/tmp` inside the L1; pass `--l1-remote-base` only when the L1 has a
different writable scratch filesystem.

The proof is green only when `process_result.exit_status == 0`,
`process_result.stdout == "hello\n"`, no Firecracker processes remain after the
run, and the temporary install root was removed.

## Common Repairs

If libvirt is unavailable, rerun:

```sh
scripts/setup-privileged-runner.sh
```

If the current shell does not see new group membership, either log out and back
in or run the L1 script from an account that can use passwordless `sudo`; the
script will use `sudo -n virsh` when the current group set cannot access
`qemu:///system`.

If cloud image download or GitHub release download fails, rerun the same
command. Both scripts use bounded curl retries and leave their work roots under
`/tank/tmp` for inspection on failure.
