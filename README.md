# m80

m80 runs one command inside a Firecracker microVM and returns stdout, stderr,
and the exit code like a normal process.

## Quickstart

On a Linux/KVM machine with `sudo`, `curl`, `python3`, `sha256sum`, and `tar`:

<!-- m80:freshness-status start -->
Public installer status: public proof green for `v0.2.11`.
<!-- m80:freshness-status end -->

<!-- m80:quickstart-snippet latest-install start -->
```sh
curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh
```
<!-- m80:quickstart-snippet latest-install end -->

Check the host:

```sh
m80 preflight
```

Run something:

<!-- m80:quickstart-snippet post-install-smoke start -->
```sh
m80 run -- echo hello
```
<!-- m80:quickstart-snippet post-install-smoke end -->

<!-- m80:quickstart-value start -->
`m80 run -- <command>` runs that process in a Firecracker microVM and returns stdout, stderr, and exit code.
<!-- m80:quickstart-value end -->

Try a few useful shapes:

```sh
m80 run -- uname -a
m80 run --egress none -- sh -c 'id && pwd'
m80 run --workspace . --cwd /workspace -- ls -la
m80 run --workspace . --cwd /workspace --writeback on-success -- sh -c 'date > m80.out'
m80 run --egress outbound -- curl -I https://example.com
m80 run --env FOO=bar -- sh -c 'echo "$FOO"'
m80 run -it --workspace . --cwd /workspace -- sh
```

Keep it current:

```sh
m80 update --check
```

If it prints a `next_command=...`, run that command.

Pin a version for automation:

<!-- m80:quickstart-snippet pinned-install start -->
```sh
curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh | sudo sh
```
<!-- m80:quickstart-snippet pinned-install end -->

Use a stable tag such as `v0.2.11`.

If something fails, start with:

```sh
m80 install-status
```

If you are coming from the old artifact-only quickstart or a raw `main`
installer command, use the
[`legacy quickstart migration note`](docs/behaviors/release/legacy-quickstart-hard-cutover.md).

Then check the host:

```sh
m80 preflight
```

Known failure IDs live in the quickstart troubleshooting matrix:
[`network`](docs/behaviors/release/quickstart-troubleshooting-matrix.md#network)
and
[`process-smoke-failed`](docs/behaviors/release/quickstart-troubleshooting-matrix.md#process-smoke-failed).

`m80 preflight` reports missing host setup before launch. Firecracker needs a
Linux/KVM host, `/dev/kvm` access, the Firecracker binary, jailer binary,
Firecracker seccomp filter, `host-binaries.manifest.json` for the installed
host-side TCB, m80 artifacts, and startup privilege through root, the documented
m80 capability set, or a privileged container. For v0.x, Firecracker, jailer,
and the Firecracker seccomp filter are operator-provided host prerequisites; if
preflight reports one of those failures, see the
[`host prerequisite policy`](docs/behaviors/release/host-prerequisite-policy.md)
and
[`host prerequisite verifier`](docs/behaviors/preflight/host-prerequisite-verifier.md).

Production operators should also read
[`docs/ops/host-setup.md`](docs/ops/host-setup.md).

## How It Works

`m80 run` wraps one process in one Firecracker microVM. The process still looks
like a normal command to the caller: stdin goes in, stdout/stderr/exit come
back, and m80 handles the VM setup and teardown around it.

## Diagnostics

`m80 run` keeps stdout/stderr transparent for the wrapped process. VM mechanics
diagnostics are captured out-of-band under the run directory and read
separately:

```sh
m80 logs <vm-id>
m80 --json logs <vm-id> --request-id req_...
```

`console.log` includes guest-influenced serial-console output and is capped at
2 MiB per VM. Review [`docs/ops/logging.md`](docs/ops/logging.md) before
shipping run-directory logs outside the host.

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

## Lifecycles

m80 supports three lifecycle modes; the adapter or caller chooses by request.
The CLI facade is `m80 run`; direct snapshot restore and persistent-VM control
are library/warm-owner surfaces, not `m80 run` compatibility flags:

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

Layered rootfs, pmem layers, Shared same-trust-domain backing, and
snapshot-template restore are covered by the operator runbooks:
[`docs/runbook/layered-rootfs-operations.md`](docs/runbook/layered-rootfs-operations.md)
and
[`docs/runbook/migrating-to-pmem-and-snapshot-restore.md`](docs/runbook/migrating-to-pmem-and-snapshot-restore.md).
The verified-close checklist for the remaining q420k measurement gates lives
at
[`docs/runbook/q420k-close-gates.md`](docs/runbook/q420k-close-gates.md).

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

Use `kind = "minimal-erofs"` to build the same busybox + static-guestd
minimal image as a compressed read-only erofs base (`output.erofs`) when the
selected kernel has built-in erofs support.

Release artifacts are produced by
[`.github/workflows/release-artifacts.yml`](.github/workflows/release-artifacts.yml)
on tag pushes.

## Workspace

m80 is a Rust workspace split into 23 black-box crates:

**Foundation (11)** — privilege acquired at process startup and verified by
`m80-preflight`; no per-call privilege shim:
- `m80-proto`, `m80-vsock` — host↔guest wire protocol + transport
- `m80-image-manifest`, `m80-image-store`, `m80-firecracker-client` — manifest schema, content-addressed artifact store, FC REST API
- `m80-jailer`, `m80-jailer-harden` — jail materialization + inheritable Group B hardening (supplementary groups, ambient caps, `no_new_privs`, signal mask, umask) wrapping FC's official jailer
- `m80-cgroup`, `m80-storage` — cgroup-v2 limits, overlay+pivot rootfs
- `m80-preflight`, `m80-net-mode` — host capability checks, network mode types

**Feature crates (4)**:
- `m80-net-outbound` — egress NAT/iptables/DNS/cleanup (~3500 LOC; the largest single risk surface)
- `m80-snapshot` — capture/restore execution
- `m80-snapshot-template` — content-addressed snapshot-template store with process-local pins
- `m80-observability` — probe + Prometheus render

**Orchestration (1)**:
- `m80-firecracker` — composes foundation crates; lifecycle state machine, run-root layout, drive hot-plug + tenant-identity verification, warm-pool / persistent-VM modes

**Binaries (4)**:
- `m80-image-build` — image construction pipeline (kernel + rootfs + guestd)
- `m80-guestd` — cross-compiled, runs as PID 1 on minimal images
- `m80-net-helper` — privileged helper for finite outbound network operations
- `m80-cli` — the `m80` binary

**Test infrastructure (3)**:
- `m80-test-helpers` — shared deterministic fixtures and assertions
- `m80-attack-runner` — malicious payload binary for defense-in-depth jailer tests
- `m80-guestd-malicious` — adversarial in-VM peer for real-KVM guest-to-host wire tests

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
- [`docs/runbook/layered-rootfs-operations.md`](docs/runbook/layered-rootfs-operations.md) — operator choices for rootfs overlays, pmem sharing, template rebuilds, and capacity
- [`docs/future-directions/bestiary-conveyor-belt.md`](docs/future-directions/bestiary-conveyor-belt.md) — multi-tenant pool architecture (out of m80 scope; consumer-side)

## What m80 Is Not

- Not Docker-compatible lifecycle UX as the primary model.
- Not an agent tool catalog or policy authority.
- Not an OCI image puller or runtime package installer.
- Not a hidden daemon. Warm execution uses an explicit resident owner.
- Not virtio-fs or shared-host-filesystem (deliberate; virtio-blk + overlay
  per-VM is the chosen storage model).
- Not single-tenant dev-ergonomics (use SmolVM for that audience).
