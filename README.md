# m80

m80 runs a process inside a Firecracker microVM while making the VM feel like a
thin host-process wrapper: stdout is stdout, stderr is stderr, the guest exit
code is the `m80` exit code, and the process sees only the filesystem, network,
environment, and runtime profile you selected.

```sh
m80 run -- echo hello
```

Expected output:

```text
hello
```

The command above uses the default runtime profile. The built-in `env` profile
reads `M80_KERNEL_IMAGE` and `M80_ROOTFS_IMAGE`; named profiles live at
`/etc/m80/profiles/<name>.toml` or `~/.config/m80/profiles/<name>.toml`.
The requested program must exist inside the selected guest image/profile or in
the visible workspace. m80 does not run host binaries, pull OCI images, or
install packages implicitly.

## Quickstart Shape

After a release artifact tarball exists, first contact is:

```sh
m80 quickstart \
  --artifact-url https://github.com/<owner>/m80/releases/latest/download/m80-linux-x86_64-minimal-artifacts.tar.gz
```

For a clean host that does not yet have the repo checkout, the same artifact
flow is available as a shell script:

```sh
curl -fsSL https://raw.githubusercontent.com/<owner>/m80/main/scripts/quickstart.sh |
  sudo sh -s -- \
    --artifact-url https://github.com/<owner>/m80/releases/latest/download/m80-linux-x86_64-minimal-artifacts.tar.gz
```

`m80 preflight` reports missing host setup before launch. Firecracker needs a
Linux/KVM host, `/dev/kvm` access, the Firecracker and jailer binaries, m80
artifacts, and startup privilege through root, file capabilities, or a
privileged container.

## Diagnostics

`m80 run` keeps stdout/stderr transparent for the wrapped process. VM mechanics
diagnostics are captured out-of-band under the run directory and read
separately:

```sh
m80 logs <vm-id>
m80 --json logs <vm-id> --request-id req_...
```

For bug reports, include a diagnostic environment dump:

```sh
m80 --json env > m80-env.json
```

That dump includes host capability checks, effective config, runtime artifact
paths, Firecracker version evidence, and run-root state. For a specific failed
VM, also attach `m80 --json logs <vm-id>`.

## Process Visibility

Common policies are direct command-line flags:

```sh
m80 run --egress none -- echo isolated
m80 run --workspace . --cwd /workspace -- ls
m80 run --workspace . --writeback on-success -- sh -c 'echo done > result.txt'
m80 run -it --workspace . --egress outbound --secret-env ANTHROPIC_API_KEY -- claude
```

- `--workspace <path>` makes one host directory visible at `/workspace`.
- `--writeback never|on-success|always` controls whether workspace mutations
  are extracted after the guest process stops.
- `--egress none|outbound` selects no network or NAT-backed outbound network.
- `--env KEY=VAL` and `--secret-env KEY` project explicit environment only.
- `--tty -i` gives the process an interactive terminal for TUIs.
- `--warm` leases from an explicit resident warm owner; it never silently falls
  back to cold boot.

See [examples](examples/) for copy-paste workloads.

## Build The Artifacts

For local development, build a minimal image:

```sh
cargo build -p m80-guestd --release --target x86_64-unknown-linux-musl
cat > /tmp/m80-image-build.toml <<'EOF'
[kernel]
version = "v1.15.1"
artifact_track = "v1.15"
arch = "x86_64"

[rootfs]
kind = "minimal"
size = "256MiB"

[guestd]
binary = "target/x86_64-unknown-linux-musl/release/m80-guestd"

[output]
dir = "/opt/m80/artifacts"
EOF
sudo target/release/m80-image-build run --config /tmp/m80-image-build.toml
```

Release artifacts are produced by
[`.github/workflows/release-artifacts.yml`](.github/workflows/release-artifacts.yml)
on tag pushes.

## Workspace

m80 is a Rust workspace split into small black-box crates:

- `m80-proto`, `m80-vsock`, `m80-firecracker-client`, `m80-jailer`,
  `m80-cgroup`, `m80-storage`, `m80-preflight`, `m80-net-mode`
- `m80-net-outbound`
- `m80-firecracker`
- `m80-image-build`, `m80-guestd`, `m80-cli`
- reserved `m80-snapshot` and `m80-observability` surfaces

Crate READMEs are contracts. A public-surface change updates the owning crate
README in the same diff.

## Positioning

m80 is the Firecracker foundation layer, not the agent product layer. It owns
the reusable VM mechanics: artifact admission, jailer setup, read-only rootfs
plus per-VM writable layers, vsock exec, PTY forwarding, direct guest file
operations, outbound networking policy, warm pools, and cleanup evidence.

Higher-level adapters can build on that surface for their own product model:

- a predecessor adapter can translate tool calls, workspace authority, and semantic
  events into m80 exec/file/network requests
- a CI runner can expose untrusted pull requests as constrained processes
- a SaaS host can isolate tenant plugins with explicit filesystem and egress
  visibility
- language bindings can wrap m80's generic process and file-operation APIs

That split is deliberate. Projects like SmolVM optimize for an SDK-shaped AI
sandbox experience. m80's edge is the audit-small Rust/Firecracker substrate:
typed Rust APIs, process-transparent CLI behavior, direct VM-mechanics tests,
and no hidden agent policy in the core. See [docs/positioning.md](docs/positioning.md)
for the fuller comparison and adapter pattern.

## What m80 Is Not

- Not Docker-compatible lifecycle UX as the primary model.
- Not an agent tool catalog or policy authority.
- Not an OCI image puller or runtime package installer.
- Not a hidden daemon. Warm execution uses an explicit resident owner.

## Dossier

The original extraction dossier is retained as historical context. Start with
`00-verdict.md` when you need the reasoning behind the crate split, risk
register, or LOC budget. The dossier is not normative; current crate READMEs
and live beads are.
