# Preflight Sentinel Cache

Date: 2026-05-14

Bead: `m80-jp6ik.17`

## Setup

Host:

- Linux 6.17.0-22-generic
- Firecracker `/opt/firecracker/bin/firecracker`, version `v1.15.1`
- Jailer `/opt/firecracker/bin/jailer`
- m80 CLI `target/release/m80`
- jailer hardening wrapper `target/release/m80-jailer-harden`
- kernel `/tmp/m80-build/minimal/vmlinux`
- rootfs `/tmp/m80-build/minimal/output.ext4`
- run root `/var/lib/m80-run`

Command environment:

```sh
M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker
M80_JAILER_BIN=/opt/firecracker/bin/jailer
M80_JAILER_HARDEN_BIN=/tank/projects/m80/target/release/m80-jailer-harden
M80_FIRECRACKER_VERSION=v1.15.1
M80_KERNEL_IMAGE=/tmp/m80-build/minimal/vmlinux
M80_ROOTFS_IMAGE=/tmp/m80-build/minimal/output.ext4
M80_RUN_ROOT=/var/lib/m80-run
```

The non-root invocation fails on the expected privilege gate:

```text
preflight: insufficient privilege; missing capabilities:
[CAP_NET_ADMIN, CAP_SYS_ADMIN, CAP_MKNOD, CAP_CHOWN, CAP_FOWNER, CAP_KILL, CAP_SETUID, CAP_SETGID, CAP_SETPCAP]
```

The timing runs therefore used `sudo env ...` so preflight could run through the
real host gates and write/read `/run/m80-preflight-ok-<sha256>`.

## Results

`hyperfine --shell=none --warmup 3 --runs 20`:

| Mode | Mean | Range | User | System |
| --- | ---: | ---: | ---: | ---: |
| cached sentinel | 6.2 ms +/- 1.0 ms | 5.6-10.0 ms | 0.6 ms | 5.6 ms |
| `M80_FORCE_PREFLIGHT=1` | 203.1 ms +/- 5.7 ms | 198.1-218.8 ms | 152.8 ms | 50.1 ms |

Cached preflight was `32.66x` faster than the forced full path on this host.

`strace -f -e execve` on the cached path showed only:

```text
execve("target/release/m80", ["target/release/m80", "--json", "preflight"], ...)
execve("/usr/bin/cp", ["/usr/bin/cp", "--reflink=always", ...], ...)
```

There was no cached-path `firecracker --version` subprocess. The cached path
also reuses the manifest from the sentinel, so it does not rehash the 256 MiB
rootfs.

## Interpretation

The sentinel cache does what the bead intended mechanically: repeated same-boot
preflight skips Firecracker version probing and rootfs manifest verification,
while host capability checks still run.

The literal `<5ms` cached-preflight target was not met on this host. The
remaining cached-path cost is dominated by checks that intentionally remain
live, especially the run-root reflink probe, which still executes
`cp --reflink=always` once per preflight. The measured result is therefore a
large practical win but a strict target miss: cached mean `6.2 ms`, minimum
`5.6 ms`, full forced mean `203.1 ms`.
