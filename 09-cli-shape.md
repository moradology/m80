# 09 — CLI shape design

## Goals

- One binary, `m80`, with subcommands
- "Low effort": common case is `m80 run -- cmd args`
- Discoverable: `m80 --help`, `m80 SUBCOMMAND --help` always work
- Honest about prerequisites: never silently downgrade or "auto-fix"
  a broken host
- Good defaults but every default overridable

## Subcommand surface

### `m80 preflight`

Run the host readiness check. Print a tabular report. Exit nonzero if
any check fails.

```
$ m80 preflight
✓ Linux host (kernel 6.12)
✓ /dev/kvm writable by current user
✓ firecracker v1.15.1 at /opt/m80/bin/firecracker
✓ jailer v1.15.1 at /opt/m80/bin/jailer
✓ kernel image at /opt/m80/artifacts/vmlinux-6.1.155 (sha256 ok)
✓ rootfs image at /opt/m80/artifacts/m80-base.ext4 (sha256 ok)
✓ manifest at /opt/m80/artifacts/m80-base.ext4.manifest.json
✓ run root at /var/lib/m80-run (writable)
✓ helpers: mkfs.ext4, debugfs, e2fsck

Host is ready.
```

### `m80 prepare-image`

Build a guest image. Driven by a recipe file or flags.

```
$ m80 prepare-image \
    --base ubuntu:22.04 \
    --install python3 nodejs \
    --output /opt/m80/artifacts/my-image.ext4

$ m80 prepare-image --recipe ./image.toml
```

Produces:
- The ext4 file
- A manifest beside it (sha256, base, install list, sizes)
- Optionally a Dockerfile-equivalent record for reproducibility

### `m80 run`

The headline command. One-shot: create, boot, exec, capture, stop,
delete. Default writeback is on if `--workspace` is given.

```
$ m80 run \
    --image /opt/m80/artifacts/my-image.ext4 \
    --kernel /opt/m80/artifacts/vmlinux-6.1.155 \
    --workspace ./src \
    --memory 1024 \
    --vcpus 1 \
    --timeout 60s \
    -- bash -c 'cd /workspace && cargo test'

# Outputs stdout/stderr to terminal, returns exit code

$ echo $?
0
```

Flags:
- `--image PATH` (required, or via `M80_DEFAULT_IMAGE` env)
- `--kernel PATH` (default: latest in `/opt/m80/artifacts/`)
- `--workspace DIR` (optional; bind-mounts at fixed guest path)
- `--writeback {auto,on,off}` (default: `auto` = on iff workspace given
  and command exits 0)
- `--memory MIB` (default: 1024)
- `--vcpus N` (default: 1)
- `--timeout DURATION` (default: 5min)
- `--network {none,outbound-nat}` (default: `none`)
- `--env KEY=VALUE` (repeatable)
- `--cwd PATH` (in-guest cwd)
- `--stdin` (pipe from caller's stdin)
- `--name NAME` (otherwise auto-generated)
- `--detach` (return immediately with VM ID; use `m80 logs/wait`)
- `--jailed` / `--no-jailed` (default: jailed in production, no-jailer
  in dev)
- `--cgroup-v2` (default: off in v0.1)
- `--keep-on-failure` (don't delete the VM if it fails; lets you debug)

### `m80 shell`

Interactive session. Boot a VM, attach a vsock-based pty, drop into a
shell. Quit detaches and stops.

```
$ m80 shell --image /opt/m80/artifacts/my-image.ext4 --workspace ./src
# logged into VM as root
# /workspace is your bind-mount
$ exit
$ # back on host; VM stopped
```

The "vsock-based pty" is the implementation detail. A simple
implementation: m80 spawns a long-running process on the guest that
allocates a pty and shuttles bytes over a side vsock channel.

### `m80 exec`

Run a command in an already-running VM (started with `m80 run --detach`
or `m80 shell --detach`).

```
$ m80 run --image foo.ext4 --detach --name dev
m80-dev-7f3a

$ m80 exec dev -- ls /workspace
# stdout
$ m80 exec dev -- bash -c 'cargo build'
$ m80 stop dev
```

### `m80 ls`

List active VMs. Reads `<run_root>/*/`.

```
$ m80 ls
NAME            STATE     UPTIME    IMAGE
dev             running   00:12:34  my-image.ext4
build-3         running   00:00:42  my-image.ext4
old-zombie      stuck     1d 04:21  my-image.ext4

$ m80 ls --json | jq '.[]'
```

### `m80 stop`

Graceful stop. Optional `--force` for hard kill.

```
$ m80 stop dev
$ m80 stop dev --force
$ m80 stop --all
```

Default behavior: send Ctrl-Alt-Del on x86_64, wait up to 30s, force-kill
on timeout. On aarch64, skip Ctrl-Alt-Del (Firecracker doesn't support
it there) and force-kill directly.

### `m80 logs`

Tail the per-VM log files (console, diagnostics, etc.).

```
$ m80 logs dev
$ m80 logs dev --follow
$ m80 logs dev --since 10m
```

In v0.1, this just tails `<run_dir>/console.log`. In v0.2, integrate
with the diagnostics jsonl if observability is enabled.

### `m80 inspect`

Show full VM state.

```
$ m80 inspect dev
name: dev
id: 7f3a...
state: running
created: 2026-05-02T14:32:11Z
image: my-image.ext4 (sha256 ok)
kernel: vmlinux-6.1.155 (sha256 ok)
memory: 1024 MiB
vcpus: 1
network: none
workspace: ./src → /workspace (rw)
run_dir: /var/lib/m80-run/7f3a...
sockets:
  api: /var/lib/m80-run/7f3a/firecracker.sock
  vsock: /var/lib/m80-run/7f3a/vsock.sock
pids:
  firecracker: 12345
  jailer: 12344
```

## Configuration loading order

1. Built-in defaults
2. `/etc/m80/config.toml` (system-wide)
3. `~/.config/m80/config.toml` (user)
4. `M80_*` environment variables
5. Command-line flags

Document this clearly. Provide `m80 config show` to reveal effective
config.

## Error reporting

Every error message includes:
- What was attempted (one short clause)
- What went wrong (one short clause)
- What the user can try next (one line)

Bad:

```
Error: spawn failed
```

Good:

```
Error: failed to spawn firecracker
  reason: /opt/m80/bin/firecracker not found
  hint:   run `m80 preflight` to check, or set M80_BIN
```

## Output format

Default: human-readable.
`--json` flag on `ls`, `inspect`, `run` (status only) for machine
consumption.

Never write banners, progress spinners, or emoji to stdout when stdin is
not a TTY. Detect with `isatty()`.

## What v0.1 does NOT do

- Multi-host orchestration (single host only)
- Image registries (image is a local file)
- Networking (NoEgress only)
- Snapshot/restore
- Warm pool / pre-booted VMs
- Resource accounting / quotas
- Auth / multi-tenancy
- Daemon mode (no `m80d`)

These are all valid future features but explicitly out of scope.

## Comparison: existing tools m80 should learn from

- **firectl** (firecracker-microvm/firectl): the official CLI. Lower
  level, doesn't manage guest daemon or workspace. m80 is opinionated
  at a higher level.
- **ignite** (weaveworks/ignite, archived but worth reading): full
  OCI-image-as-VM. m80 is simpler, doesn't try to be a Docker drop-in.
- **flintlock** (liquidmetal/flintlock): cluster-oriented. m80 is single
  host.

m80's niche: developer-friendly, opinionated single-host
"Firecracker-as-a-better-container", with workspace bind-mount and
clean workspace writeback as first-class.
