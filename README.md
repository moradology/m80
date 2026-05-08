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

## Quickstart

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
- `--persist` keeps a single VM alive across multiple `m80 run` invocations
  for sequential exec workflows.
- `--from-snapshot <path>` boots from a previously captured snapshot for fast
  warm-start.

See [examples](examples/) for copy-paste workloads.

## Lifecycles

m80 supports three lifecycle modes; the adapter or caller chooses by request:

- **Cold run** — clean state, highest latency, simplest isolation. ~1.1 s P50
  on minimal stripped images (kernel boot dominates; perf attack tracked in
  `docs/perf/cold-launch.md`).
- **Warm restore** — load a captured snapshot, skip kernel boot. ~270 ms P50;
  the building block for warm pools. See `docs/behaviors/snapshot/`.
- **Persistent VM** — single VM serves multiple sequential execs with shared
  workspace state. Useful for build/test cycles where each step depends on
  prior state. See `docs/behaviors/lifecycle/persistent-state.md`.

Drive hot-plug + tenant-identity verification primitives are available for
multi-tenant pool callers; see `docs/future-directions/bestiary-conveyor-belt.md`
for the canonical pool architecture this enables.

## File Operations

Beyond exec, m80 exposes wire verbs for direct guest file movement that avoid
shell-quoting and process-spawn overhead:

- `read_file`, `write_file`, `list_dir`, `stat_file`, `remove_file`, `mkdir`
- Chunked upload via `file_write_begin` / `file_write_chunk` / `file_write_commit`

These map to typed `FileError` responses, not stderr-text + exit-code, so
callers can pattern-match failure modes (`PermissionDenied`, `NotFound`, etc.).
Exec remains the right primitive for arbitrary workflows; file verbs are for
host-driven workspace manipulation in hot paths. See
`docs/adapter-boundary.md` "Promotion Bar For New Core Verbs" for the rubric
governing what becomes a core wire verb.

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

m80 is a Rust workspace split into 19 black-box crates:

**Foundation (10)** — privilege acquired at process startup and verified by
`m80-preflight`; no per-call privilege shim:
- `m80-proto`, `m80-vsock` — host↔guest wire protocol + transport
- `m80-image-manifest`, `m80-firecracker-client` — manifest schema, FC REST API
- `m80-jailer`, `m80-jailer-harden` — jail materialization + inheritable Group B hardening (supplementary groups, ambient caps, `no_new_privs`, signal mask, umask) wrapping FC's official jailer
- `m80-cgroup`, `m80-storage` — cgroup-v2 limits, overlay+pivot rootfs
- `m80-preflight`, `m80-net-mode` — host capability checks, network mode types

**Feature crates (3)**:
- `m80-net-outbound` — egress NAT/iptables/DNS/cleanup (~3500 LOC; the largest single risk surface)
- `m80-snapshot` — capture/restore execution
- `m80-observability` — probe + Prometheus render

**Orchestration (1)**:
- `m80-firecracker` — composes foundation crates; lifecycle state machine, run-root layout, drive hot-plug + tenant-identity verification, warm-pool / persistent-VM modes

**Binaries (3)**:
- `m80-image-build` — image construction pipeline (kernel + rootfs + guestd)
- `m80-guestd` — cross-compiled, runs as PID 1 on minimal images
- `m80-cli` — the `m80` binary

**Test infrastructure (2)**:
- `m80-test-helpers` — shared deterministic fixtures and assertions
- `m80-attack-runner` — malicious payload binary for defense-in-depth jailer tests

Crate READMEs are contracts. A public-surface change updates the owning crate
README in the same diff.

## Positioning

m80 is the Firecracker foundation layer, not the agent product layer. It owns
the reusable VM mechanics: artifact admission, jailer setup, read-only rootfs
plus per-VM writable layers, vsock exec + PTY forwarding, direct guest file
operations, outbound networking policy, warm/persistent VM lifecycles, drive
hot-plug, and cleanup evidence.

Higher-level adapters can build on that surface for their own product model:

- a tool-call adapter can translate semantic operations, workspace authority,
  and product events into m80 exec/file/network requests
- a CI runner can expose untrusted pull requests as constrained processes
- a multi-tenant pool orchestrator can use the warm-pool + drive-hot-plug
  primitives to serve isolated tenants from a shared VM pool
- language bindings can wrap m80's typed APIs

That split is deliberate. SmolVM optimizes for an SDK-shaped single-tenant
sandbox experience (in-process VMM, virtio-fs, no jailer). m80's edge is the
audit-small Rust/Firecracker substrate optimized for **multi-tenant**
deployments: typed Rust APIs, process-transparent CLI behavior, direct
VM-mechanics tests, mandatory jailer, and no hidden agent policy in the core.

See:
- [`docs/positioning.md`](docs/positioning.md) — fuller comparison and adapter pattern
- [`docs/adapter-boundary.md`](docs/adapter-boundary.md) — what lives in m80 vs above (with the Promotion Bar For New Core Verbs)
- [`docs/future-directions/bestiary-conveyor-belt.md`](docs/future-directions/bestiary-conveyor-belt.md) — multi-tenant pool architecture (out of m80 scope; consumer-side)

## What m80 Is Not

- Not Docker-compatible lifecycle UX as the primary model.
- Not an agent tool catalog or policy authority.
- Not an OCI image puller or runtime package installer.
- Not a hidden daemon. Warm execution uses an explicit resident owner.
- Not virtio-fs or shared-host-filesystem (deliberate; virtio-blk + overlay
  per-VM is the chosen storage model).
- Not single-tenant dev-ergonomics (use SmolVM for that audience).
