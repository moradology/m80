# m80

m80 runs a process inside a Firecracker microVM while making the VM feel like a
thin host-process wrapper: stdout is stdout, stderr is stderr, the guest exit
code is the `m80` exit code, and the process sees only the filesystem, network,
environment, and runtime profile you selected.

<!-- m80:quickstart-snippet post-install-smoke start -->
```sh
m80 run -- echo hello
```
<!-- m80:quickstart-snippet post-install-smoke end -->

Expected output:

```text
hello
```

The command above uses the default runtime profile. The release installer
writes `/etc/m80/profiles/default.toml` plus `/etc/m80/config.toml` so the
default profile points at the installed guest bundle. The built-in `env`
profile remains available for explicit environment-driven development.
The requested program must exist inside the selected guest image/profile or in
the visible workspace. m80 does not run host binaries, pull OCI images, or
install packages implicitly.

## Quickstart

This buys you a Firecracker-backed process wrapper: `m80 run -- <command>`
boots a microVM, runs the command, streams stdout/stderr back like a normal
process, returns the guest exit code, and tears the VM down.

Public installer status: pending until the unauthenticated public-access proof
is green for the promoted release. The command below is the release-channel
template; use it only after that proof is green.
<!-- m80:public-access-proof m80-o3uh9.21.7 pending -->

<!-- m80:quickstart-snippet latest-install start -->
```sh
curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh
```
<!-- m80:quickstart-snippet latest-install end -->

```sh
m80 run -- echo hello
```

The install snippets are checked against the public release URL contract in
[`docs/behaviors/release/public-release-root.env`](docs/behaviors/release/public-release-root.env)
by [`scripts/render-release-install-snippets.py`](scripts/render-release-install-snippets.py)
and the marker gate in
[`docs/behaviors/release/docs-quickstart-gate.md`](docs/behaviors/release/docs-quickstart-gate.md).
The `latest` command follows the stable release channel only: public,
non-draft, non-prerelease GitHub releases tagged `vMAJOR.MINOR.PATCH`, and it
is not considered promoted until the unauthenticated public-access proof is
green.

The installer is a rendered asset from the selected release. It uses that
pinned release tag to download the release asset index and shell-safe bootstrap
selector, selects the matching Linux host bundle, verifies the signed release
integrity predicate and checksums before extraction, then runs the bundled
`m80 install`. The installer needs standard Linux tools plus `curl`,
`python3`, `sha256sum`, `tar`, and GitHub CLI `gh` with
`gh attestation verify`. The install writes the default runtime profile/config
so plain `m80 run -- echo hello` uses the installed guest bundle. Host TCB
binaries are installed separately from final host paths; release tarballs must
not bundle the host manifest. The bundle shape is pinned in
[`docs/behaviors/release/bundle-contract.md`](docs/behaviors/release/bundle-contract.md).

To see what the next `m80 run` will use, run:

```sh
m80 install-status
```

It reports the active release tag, active install directory, selected
profile/config, installed metadata paths, and one next action when the install
is missing or stale. `m80 --json install-status` is the stable machine-readable
form for scripts and freshness checks.

To check whether the installed release is still current without touching the
install root, run:

```sh
m80 update --check
```

It reports finite states including `current`, `outdated`, `unknown_offline`,
`stale_latest_metadata`, `prerelease_active`, `ineligible_active`,
`local_dev_install`, `install_unhealthy`, `yanked`, and `unsafe`, and prints the
exact pinned install command when a newer safe release is known.

After that, wrap any process the same way:

```sh
m80 run -- <command> [args...]
```

For reproducible automation, replace `latest` with a concrete tag:

<!-- m80:quickstart-snippet pinned-install start -->
```sh
curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh | sudo sh
```
<!-- m80:quickstart-snippet pinned-install end -->

```sh
m80 run -- echo hello
```

Use a stable tag such as `v1.2.3`; prerelease or draft releases are not accepted
by the normal install path.

Automation from a trusted checkout can verify the installer before handing it to
sudo:

<!-- m80:quickstart-snippet verified-install-handoff start -->
```sh
tag=<version>
repo=moradology/m80
tmp="$(mktemp -d)"
base="https://github.com/${repo}/releases/download/${tag}"
for asset in install.sh install.sh.sha256 m80-release-integrity.json m80-release-integrity.attestation.jsonl m80-release-attestation.json; do
  curl -fsSLo "${tmp}/${asset}" "${base}/${asset}"
done
python3 scripts/verify-install-handoff.py "${tmp}" \
  --release-tag "${tag}" \
  --trust-policy docs/behaviors/release/m80-release-trust-policy.json \
  --verification-time "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
sudo sh "${tmp}/install.sh"
```
<!-- m80:quickstart-snippet verified-install-handoff end -->

If install, preflight, or the first `m80 run -- echo hello` fails, capture
`m80 install-status` first and then `m80 preflight`; status explains local
install-state problems before you debug a launch: missing active pointer, stale
profile target, explicit profile override, local-dev profile, missing/stale
install metadata, and tampered proof cache.
Re-running install with a known older stable release is refused by default
before staging or profile writes; the diagnostic names the active tag, requested
tag, `downgrade_refused`, and a pinned reinstall command for the active release.
`m80 preflight` reports missing host setup before launch. Firecracker needs a
Linux/KVM host, `/dev/kvm` access, the Firecracker binary, jailer binary,
Firecracker seccomp filter, `host-binaries.manifest.json` for the installed
host-side TCB, m80 artifacts, and startup privilege through root, the documented
m80 capability set, or a privileged container. For v0.x, Firecracker, jailer,
and the Firecracker seccomp filter are operator-provided host prerequisites; if
preflight reports one of those failures, use the short
[`host prerequisite policy`](docs/behaviors/release/host-prerequisite-policy.md)
to decide what to install or repair. The structured repair fields are defined
by the
[`host prerequisite verifier`](docs/behaviors/preflight/host-prerequisite-verifier.md)
contract.

If `m80 install` fails with `asset_index_code=...`, the requested
OS/architecture/image kind, m80 version, available release tuples, and exact
repair command are defined in
[`docs/behaviors/release/asset-index.md`](docs/behaviors/release/asset-index.md).

Production operators should read [`docs/ops/host-setup.md`](docs/ops/host-setup.md)
before trusting a host. It covers identity separation, Docker socket risk,
artifact ownership, Cargo source controls, and runtime host assumptions.
Developers running the privileged real-KVM test battery locally should use
[`docs/operations/e2e-local-dev.md`](docs/operations/e2e-local-dev.md).

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

m80 is a Rust workspace split into 22 black-box crates:

**Foundation (10)** — privilege acquired at process startup and verified by
`m80-preflight`; no per-call privilege shim:
- `m80-proto`, `m80-vsock` — host↔guest wire protocol + transport
- `m80-image-manifest`, `m80-firecracker-client` — manifest schema, FC REST API
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
