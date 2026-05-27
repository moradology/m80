# Changelog

All notable changes to m80 are documented here. Format roughly follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Added a human-reviewed release-notes pipeline: `/draft-release-notes` drafts
  candidate changelog text for operator review, `scripts/prep-release.sh`
  promotes reviewed `[Unreleased]` notes to a versioned section, and GitHub
  Releases now publish the committed `CHANGELOG.md` section through
  `--notes-file`.
- Added changelog discipline for pull requests, including an advisory workflow
  warning and PR-template checkbox so release-relevant changes keep
  `[Unreleased]` current before release prep.
- Added historical v0.2 changelog backfill tooling: a reviewed packet,
  structural/git-range verifier, landing helper, and release-notes proof
  verifier for the first future tag cut through the new pipeline.

### Changed

- Hardened privileged E2E routing so real-KVM jobs require broker-issued runner
  labels and cannot silently fall back to the persistent `kvm` runner.
- Updated privileged E2E target-state docs and public freshness proof handling
  for post-publication release evidence.

## [v0.2.25] — 2026-05-27

v0.2.25 carries the proved current-latest release state forward and keeps
privileged measurement benchmarks out of ordinary CI/test builds.

### Changed

- Marked the public installer freshness proof green for `v0.2.24` in the README
  and release runbook, including unauthenticated latest/pinned install URL
  evidence.
- Advanced the current-latest repair source contract to workspace version
  `0.2.25`, with `v0.2.24` treated as the existing public latest tag for repair
  preflight ordering checks.
- Gated real-KVM Cargo benchmark binaries behind the `real-kvm-bench` feature
  so `cargo test --workspace --all-targets` does not build privileged
  measurement programs unless explicitly requested.

### Fixed

- Updated snapshot-template benchmark documentation and verifier expectations so
  real-KVM benchmark reproduction commands include `--features real-kvm-bench`.

## [v0.2.24] — 2026-05-27

v0.2.24 restores the complete flat binary projection for systemd-selected hosts.

### Fixed

- Always publish the flat `m80-jailer-harden` binary so default preflight and
  installer-owned host-binaries manifests agree on the stable `/opt/m80/bin`
  layout.

## [v0.2.23] — 2026-05-27

v0.2.23 closes the systemd-first launch and Firecracker final-exec
investigation work, while updating privileged E2E proof targets.

### Added

- Added the systemd-first launch closeout, documentation, ignored-test taxonomy,
  capability review, and net-helper systemd launch closure.
- Added the Phase 2 Firecracker final-exec investigation docs, upstream path
  proposal, consumer/maintainer survey, and go-forward ADR.

### Changed

- Aligned the systemd ADR with the final-exec investigation and refreshed
  command inventory/proof state.
- Bumped workspace version to `0.2.23` for the release cut.

## [v0.2.22] — 2026-05-27

v0.2.22 fixes a legacy flat-cache installer backup case.

### Fixed

- Preserved legacy flat release-proof-cache content during install backup so old
  nested cache state no longer blocks activation.

## [v0.2.21] — 2026-05-27

v0.2.21 introduces the flat installed layout and advances privileged E2E toward
disposable, broker-addressed real-KVM runs.

### Added

- Added the flat `/opt/m80/bin` and `/opt/m80/artifacts` hardlink projection so
  clients and preflight can use stable paths while versioned install records
  remain available for rollback.
- Added privileged E2E runner work for ephemeral L1 provisioning, target-SHA
  checkout, runner-label naming, anonymous checkout, and proof capture.
- Added release/latest freshness proof state and public CI checkout contract
  updates around the privileged E2E path.

### Changed

- Updated installer metadata and manifests to record flat client-facing paths
  while preserving versioned install records.
- Hardened disposable privileged E2E setup and aligned CI checkout behavior for
  public and privileged workflows.

### Fixed

- Fixed per-thread `CAP_NET_ADMIN` dropping and related privileged test setup
  issues found while proving the flat installer path.

## [v0.2.20] — 2026-05-23

v0.2.20 restores lockfile correctness while keeping the installer fixes from the
prior release line.

### Fixed

- Restored the lockfile after the v0.2.19 version bump accidentally rewrote the
  locked `libc` entry.

## [v0.2.19] — 2026-05-23

v0.2.19 retries transient lag in latest-release receipt generation.

### Fixed

- Added retry handling for transient stale-latest observations before declaring
  public-access receipt failure.

## [v0.2.18] — 2026-05-23

v0.2.18 preserves command lookup behavior inside the public installer.

### Fixed

- Preserved the public installer's configured command directory inside its
  sanitized child environment so installed command lookup works correctly.

## [v0.2.17] — 2026-05-23

v0.2.17 moves real-KVM proof out of the hosted latest-promotion gate.

### Changed

- Shifted real-KVM proof collection to closeout/freshness evidence so hosted
  latest promotion no longer depends on external real-KVM receipts.

## [v0.2.16] — 2026-05-23

v0.2.16 fixes release authority checks before draft creation.

### Fixed

- Treated a missing release as the expected pre-create state while preserving
  hard failures for unreadable release API metadata.

## [v0.2.15] — 2026-05-23

v0.2.15 broadens acceptable release protection proof for the hosted workflow.

### Fixed

- Accepted an active branch ruleset with required status checks as the
  workflow-readable proof for protected release publication.

## [v0.2.14] — 2026-05-23

v0.2.14 improves repository protection auditing for release tags.

### Fixed

- Hydrated release tag ruleset details before evaluating tag-protection
  readiness for release publication.

## [v0.2.13] — 2026-05-23

v0.2.13 repairs the release publish artifact handoff.

### Fixed

- Flattened the release artifact handoff so the publish job consumes the
  verified upload root rather than a nested artifact directory.

## [v0.2.12] — 2026-05-23

v0.2.12 closes out the release-install epoch. It improves the public installer,
gives operators better diagnostics for install/update failures, and records the
proof artifacts needed to trust latest, freshness, and release-readiness state.

### Added

- Added the redacted `m80 bug-report` support bundle for collecting operator
  diagnostics without leaking sensitive host data.
- Added quickstart troubleshooting taxonomy, generated troubleshooting
  matrices, and coverage reports for common install/update failure modes.
- Added install transaction proofing, install cleanup support, lock repair
  guards, path canonicalization, selector preservation, extraction sandboxing,
  host-binary manifest verification, and release-binary PATH installation.
- Added release evidence artifacts for public install/status proof, freshness
  drift and repair commands, provenance contents, public assets, latest
  promotion, readiness decisions, repository protection, and publish authority.
- Added freshness/update status rendering and safety-floor validation so
  operators can distinguish stale latest, offline cache, policy failures, and
  repairable local state.

### Changed

- Split installer, quickstart, environment, and preflight code into smaller
  modules while keeping the public installer-first workflow.
- Refreshed release runbooks, command inventories, proof digests, and
  CI/workflow policy checks around the public install path.

### Fixed

- Fixed release proof citations, troubleshooting proof envelopes, install
  handoff identity checks, upgrade/rollback atomicity, and freshness-status
  documentation drift found while closing the release-install proof graph.

## [v0.2.11] — 2026-05-22

v0.2.11 hardens generated VM identifier handling before launch.

### Fixed

- Added generated VM id checks before launch to catch path-budget issues before
  Firecracker setup mutates host state.

## [v0.2.10] — 2026-05-22

v0.2.10 fixes default release install selector placement.

### Fixed

- Used host selector state for default release installs so generated install
  metadata lands in the expected host location.

## [v0.2.9] — 2026-05-22

v0.2.9 repairs latest-promotion proof ordering.

### Fixed

- Proved the latest release state after promotion instead of before promotion,
  matching the public release channel's observable state.

## [v0.2.8] — 2026-05-22

v0.2.8 completes current-latest repair proofing and preserves public installer
proof-cache material.

### Added

- Recorded public-access and current-latest publish proof artifacts for the
  release repair path.

### Changed

- Preserved proof-cache material from the public installer and made repair
  proofs policy-complete for current-latest validation.

## [v0.2.7] — 2026-05-22

v0.2.7 is the big hardening and public-install release. It replaces the wire
protocol, expands real-KVM and adversarial coverage, strengthens
guest/jailer/network isolation, and adds the release/latest proof machinery
needed for public installer confidence.

### Added

- Added protobuf length-prefixed framing and typed protocol surfaces for process
  execution, file operations, PTY, streaming output, cancellation, health
  checks, and hotplug coordination.
- Added broad real-KVM and fixture coverage for launch, file operations,
  streaming, PTY, cancellation, idle timeout, warm pools, snapshot restore,
  protocol rejection, storage overlays, outbound networking, cgroups, jailer
  recovery, and malicious guest behavior.
- Added OutboundNat and JoinNetns networking surfaces, Firecracker drive
  PATCH/hotplug support, guest uevent waiting, verified drive attach flow, and
  stronger network setup/teardown recovery.
- Added guestd hardening for host liveness, Ping/Pong health, CPU/memory
  hotplug events, PID-1 storage repair fallback, bounded stdin payloads, and
  cancellation crash paths.
- Added jailer/security parity work covering cgroup setup, daemonized launch,
  private Firecracker executable handling, jail recovery, attack-runner
  harnesses, and isolation behavior docs.
- Added stable public install machinery: asset indexes, installer verification,
  freshness/update status, direct URL verification, proof ledgers, workflow
  policy checks, and public release receipt validation.

### Changed

- Hard-cut the host/guest wire protocol to protobuf framing, with no active
  NDJSON compatibility path.
- Normalized crate APIs, READMEs, behavior docs, and test helpers through the
  cleanup/refactor passes that followed the protocol and isolation work.

### Fixed

- Fixed launch, cleanup, cancellation, storage, networking, cgroup, jailer, and
  release-publication regressions discovered by the expanded proof and
  adversarial test surface.

## [v0.2.6] — 2026-05-06

v0.2.6 is a CI-quality repair release.

### Fixed

- Fixed clippy warnings that blocked the CI release path.

## [v0.2.5] — 2026-05-06

v0.2.5 narrows the quickstart probe so it works without network egress.

### Fixed

- Ran the quickstart probe without requiring outbound network access, keeping
  first-run validation local to the installed release.

## [v0.2.4] — 2026-05-06

v0.2.4 repairs CI and smoke configuration for the release path.

### Fixed

- Fixed CI lint issues and smoke image configuration so release validation uses
  the intended image setup.

## [v0.2.3] — 2026-05-06

v0.2.3 fixes quickstart install layout after package extraction.

### Fixed

- Relocated the quickstart manifest after install so installed quickstart
  commands can find the manifest in the expected location.

## [v0.2.2] — 2026-05-06

v0.2.2 separates image artifact identity from Firecracker version identity in
the release/install path.

### Changed

- Split the image artifact track from the Firecracker version track so release
  metadata can distinguish guest image artifacts from the host VMM version.

## [v0.2.1] — 2026-05-06

v0.2.1 repairs the release workflow after the initial v0.2.0 cut.

### Fixed

- Fixed the release workflow's musl target installation path so release
  artifact builds can install the expected target toolchain.

## [v0.2.0] — 2026-05-06

v0.2.0 is the first release where m80 becomes a usable process wrapper rather
than a Firecracker skeleton. It adds `m80 run`, quickstart examples, minimal
boot images, guest process IO, and the early lifecycle/performance machinery
used by later releases.

### Added

- Added the `m80 run` product surface, including workspace visibility,
  environment handling, stdout/stderr capture, exit-code propagation,
  config/profile loading, and quickstart examples.
- Added guest process execution support for streaming IO, PTY-style
  interaction, cancellation, idle timeout handling, and warm-owner lifecycle
  plumbing.
- Added minimal-image build support, image/kernel-kind manifest fields,
  stripped-kernel build work, guest PID-1 mode, and workspace mount setup inside
  the guest.
- Added snapshot capture/restore primitives, persistent execution groundwork,
  warm-pool groundwork, per-phase timing, and benchmark tooling for
  launch-latency work.
- Added sparse rootfs overlays, guest overlay mount/pivot behavior, storage
  layout changes, and vsock-based readiness/shutdown paths.

### Changed

- Reworked the docs and release surface around m80 as a generic process sandbox
  instead of exposing raw Firecracker mechanics as the main workflow.
- Tightened ready-probe cadence and removed the earlier smoke retry loop.

### Fixed

- Fixed guestd boot ordering and readiness by moving from systemd-cycle-prone
  startup to host-observable vsock readiness.

## [v0.1.0-smoke-passing] — 2026-05-04

End-to-end microVM launch works on KVM. `m80 launch -- /bin/echo hello`
boots a firecracker VM, runs the command, and exits cleanly.

### Added

- **`m80` CLI** — `preflight`, `launch`, `exec`, `cleanup`, `inspect`,
  `list`, `stop`, `config show`, `version`. Stable per-class exit
  codes (`EXIT_PREFLIGHT=2`, `EXIT_ADMISSION=3`, `EXIT_MANIFEST=4`,
  `EXIT_INVALID_STATE=5`, `EXIT_CONFIG=6`, `EXIT_NOT_IMPLEMENTED=7`),
  `--json` envelope mode for scripted callers.
- **`m80-firecracker`** — 12-phase preboot pipeline composing every
  foundation crate. Type-state lifecycle (Sandbox → RunningSandbox →
  StoppedSandbox), admission semaphore with permit return on Drop,
  `Backend::recover_stale_run_root()` that handles dead-jail / orphan-
  firecracker / no-jail cases (kills orphans, unmounts via mountinfo,
  removes cgroup leaf, rms run-dir).
- **`m80-image-build`** — pulls kernel + rootfs from firecracker-ci S3,
  loop-mounts and installs `m80-guestd` + systemd units, emits provenance
  manifest. Subcommands: `run`, `verify`, `clean`.
- **`m80-preflight`** — 10-check capability gate (KVM, kernel modules,
  privilege caps, firecracker+jailer binaries, kernel image, rootfs +
  manifest, run-root with `nodev` rejection, storage helpers, OS gate).
- **`m80-jailer`** — bind-plan + materialize + jailer launch + stale
  recovery. Source split: `types.rs`, `plan.rs`, `materialized.rs`,
  `recover.rs`, `error.rs`.
- **`m80-firecracker-client`** — sync HTTP-over-UDS REST client with
  typed per-endpoint errors.
- **`m80-cgroup`** — per-VM cgroup v2 subtree under `m80-firecracker/`.
- **`m80-storage`** — rootfs clone + scratch ext4 create/hydrate +
  post-stop change extraction.
- **`m80-vsock`** — host-side bridge with `Channel::open_uds_only` for
  callers that establish guest readiness without a console-log file.
- **`m80-proto`** — wire schemas (`Envelope<T>`, `ExecRequest`,
  `ExecResponse`, `ExecStatus`, `ExecTiming`, `HandshakeMessage`),
  length-prefixed framing.
- **`m80-image-manifest`** — guest-image provenance schema with
  `Manifest::verify` (recomputes sha256 of every artifact).
- **`m80-net-mode`** — `NetworkPolicy` (caller intent) → `VmNetworkMode`
  (resolved) split. `OutboundNat` deferred to v0.2.
- **`m80-guestd`** — in-VM daemon binary. Listens on vsock port 9001,
  serves one `Envelope<ExecRequest>` per connection.
- **`scripts/smoke.sh`** — runnable end-to-end smoke test capturing the
  env-var dance.
- **CI** — minimal GitHub Actions workflow: fmt-check, build, test,
  clippy on push + PR to main.

### Smoke-test fixes

Discovered while running the first end-to-end launch on a real KVM host:

- **Jailer chroot path** — `m80-jailer` was placing bind mounts in
  `<run_dir>/jail/`, but the official jailer binary creates its chroot
  at `<run_dir>/<exec basename>/<id>/root/`. New `chroot_path()` helper
  derives jailer's actual layout.
- **`--api-sock` placement** — was passed as a jailer arg; firecracker
  rejected. Moved past the `--` separator so jailer forwards it.
- **No `--daemonize`** — without it, jailer `exec()`s into firecracker
  so the spawned `Child` handle's pid IS firecracker. The previous
  `child.wait()` blocked forever waiting for the VM to exit. Switched
  to `mem::forget(child)` + Drop-side kill+reap.
- **RW bind chown** — bind-mounted `rootfs.ext4` was owned by host root;
  jailed firecracker (uid 3000) couldn't open it for writing. RW binds
  now `chown` the source to the jail uid:gid.
- **`/tmp` is `nodev`** — the kernel forbids opening device nodes on a
  `nodev` filesystem regardless of file permissions, so `/dev/kvm`
  inside a chroot under `/tmp` failed with EACCES on `InstanceStart`.
  `m80-preflight` now rejects `nodev` run-roots up-front. Default
  switched to `/var/lib/m80-run`.
- **Vsock `open_uds_only`** — `Channel::open` watches a console-log
  file for the ready marker first; that watch was returning
  `NotReady` and short-circuiting the UDS connect. Added
  `Channel::open_uds_only` for callers that establish readiness via
  another channel (the orchestrator's polling loop on the UDS itself).
- **Image-build alignment** — the `firecracker-ci` S3 bucket layout
  changed; updated kernel + rootfs paths to match what's actually
  shipped today (`vmlinux-5.10.245`, `ubuntu-24.04.squashfs`).
- **Mkfs target sizing** — `mkfs.ext4 -d srcdir target` requires the
  target file pre-allocated; now `truncate`d before mkfs.
- **Manifest daemon-binary path** — was the in-VM destination, but
  `Manifest::verify` walks every path on the host filesystem.
  `m80-image-build` now writes a host audit copy and references it.
- **Stale-recovery teardown** — `recover_stale_run_root` previously
  failed with EBUSY on bind mounts left by SIGKILL'd VMs. New helper
  reads `/proc/self/mountinfo` and `umount2(MNT_DETACH)`s every
  mountpoint at-or-below the stale run-dir before `rm -rf`. Also
  cleans the cgroup leaf, kills any orphaned firecracker process.

### Trimmed

A 16-crate / one workspace-pass `/trim-the-fat` sweep removed ~2,150
lines of cosmetic ceremony, dead surface, and stale documentation:

- Forbidden `#[non_exhaustive]` on internal `ExecStatus` (CLAUDE.md).
- Dead `ExecRequest::workspace_dir` wire field (no producer, no consumer).
- Deferred-v0.2 `put_network_interface` / `NetworkInterfaceConfig` (will
  return when `OutboundNat` lands).
- Many narrative comments restating visible code; many README sections
  duplicating rustdoc; many ASCII section dividers.
- Stale comments left over from the smoke-test fixes (jailer reaping,
  chroot path, `--daemonize` semantics).
- Lossy `map_err(|_|)` sites in config parsing + preflight that hid
  underlying error context.
- DRY: `SnapshotManifest`/`RestoreMetadata` write/read into shared
  helpers; `m80-storage` mount/umount triplicate into one helper;
  `Channel::close`/`Drop` into one `teardown`.

### Project state at this tag

- 16 crates, ~7,000 LOC src.
- All workspace tests pass (modulo `m80-guestd::exec_with_timeout_returns_timed_out`
  timing flake under heavy parallel load — passes solo, mitigated in CI
  with `--test-threads=2`).
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo fmt --all -- --check` clean.
- End-to-end verified by `scripts/smoke.sh`.

### Deferred to v0.2

- `m80-net-outbound` — bridge/tap/iptables/DNS for `OutboundNat` mode.
  v0.1 rejects `OutboundNat` in phase 6 with a clear error.
- `m80-snapshot` capture/restore execution — schemas + persistence
  paths active; `capture()`/`restore()` return `Deferred`.
- `m80-observability` Probe + `render_prometheus` execution — types
  pinned, execution returns `Deferred`. `Diagnostics::disabled()` works.
- Warm pool / persistent-state lifecycle (epic m80-rrp).
- `m80-adapter` agent-semantics layer on top of m80-firecracker (epic
  m80-qokt.1).
- `m80 exec` out-of-process IPC (the CLI subcommand returns
  `EXIT_NOT_IMPLEMENTED` in v0.1; library callers use
  `RunningSandbox::exec()` directly).

## [pre-v0.1] — 2026-04-29 to 2026-05-03

Bead-capture phase + initial implementation waves. See `git log` for
detail; relevant commits: `c738ba7` (initial bootstrap), `8333052`
(wave-1b: 7 foundation crates), `4403ea9` (wave-2-3: cgroup,
firecracker orchestrator, cli).
