# v0.2 Backfill Review Packet

Status: draft suggestions only. Human review/refinement is still required before
any section below lands in `CHANGELOG.md`.

Generated: 2026-05-27

Required stable tag range: v0.2.0 through v0.2.25 (captured at leaf claim time).

Source command shape:

```sh
git log --no-merges --format='%h %s%n%b' <previous-tag>..<tag>
git diff --stat <previous-tag>..<tag>
```

`br changelog` is not used as the canonical source for this packet because
`docs/exploration/br-changelog-usability.md` found it is closure-time based,
not commit-range based.

Human reviewer checklist for each section:

- [ ] Verify the bullets against `git log <previous-tag>..<tag>`.
- [ ] Remove bullets that are too internal for release notes.
- [ ] Rewrite wording for user/operator clarity.
- [ ] Check whether the existing `[Unreleased]` text has better phrasing.
- [ ] Land only reviewed text in `CHANGELOG.md`.

## Coverage Table

| Tag | Date | Range | Commits | Review status |
|---|---:|---|---:|---|
| v0.2.0 | 2026-05-06 | v0.1.0-smoke-passing..v0.2.0 | 65 | human-reviewed by nathan on 2026-05-27 |
| v0.2.1 | 2026-05-06 | v0.2.0..v0.2.1 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.2 | 2026-05-06 | v0.2.1..v0.2.2 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.3 | 2026-05-06 | v0.2.2..v0.2.3 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.4 | 2026-05-06 | v0.2.3..v0.2.4 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.5 | 2026-05-06 | v0.2.4..v0.2.5 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.6 | 2026-05-06 | v0.2.5..v0.2.6 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.7 | 2026-05-22 | v0.2.6..v0.2.7 | 755 | human-reviewed by nathan on 2026-05-27 |
| v0.2.8 | 2026-05-22 | v0.2.7..v0.2.8 | 9 | human-reviewed by nathan on 2026-05-27 |
| v0.2.9 | 2026-05-22 | v0.2.8..v0.2.9 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.10 | 2026-05-22 | v0.2.9..v0.2.10 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.11 | 2026-05-22 | v0.2.10..v0.2.11 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.12 | 2026-05-23 | v0.2.11..v0.2.12 | 224 | human-reviewed by nathan on 2026-05-27 |
| v0.2.13 | 2026-05-23 | v0.2.12..v0.2.13 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.14 | 2026-05-23 | v0.2.13..v0.2.14 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.15 | 2026-05-23 | v0.2.14..v0.2.15 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.16 | 2026-05-23 | v0.2.15..v0.2.16 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.17 | 2026-05-23 | v0.2.16..v0.2.17 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.18 | 2026-05-23 | v0.2.17..v0.2.18 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.19 | 2026-05-23 | v0.2.18..v0.2.19 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.20 | 2026-05-23 | v0.2.19..v0.2.20 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.21 | 2026-05-27 | v0.2.20..v0.2.21 | 101 | human-reviewed by nathan on 2026-05-27 |
| v0.2.22 | 2026-05-27 | v0.2.21..v0.2.22 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.23 | 2026-05-27 | v0.2.22..v0.2.23 | 31 | human-reviewed by nathan on 2026-05-27 |
| v0.2.24 | 2026-05-27 | v0.2.23..v0.2.24 | 1 | human-reviewed by nathan on 2026-05-27 |
| v0.2.25 | 2026-05-27 | v0.2.24..v0.2.25 | 2 | human-reviewed by nathan on 2026-05-27 |

## Draft Sections

### v0.2.0

Range: `v0.1.0-smoke-passing..v0.2.0`

Human-reviewed by nathan on 2026-05-27

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

### v0.2.1

Range: `v0.2.0..v0.2.1`

Human-reviewed by nathan on 2026-05-27

## [v0.2.1] — 2026-05-06

v0.2.1 repairs the release workflow after the initial v0.2.0 cut.

### Fixed

- Fixed the release workflow's musl target installation path so release
  artifact builds can install the expected target toolchain.

### v0.2.2

Range: `v0.2.1..v0.2.2`

Human-reviewed by nathan on 2026-05-27

## [v0.2.2] — 2026-05-06

v0.2.2 separates image artifact identity from Firecracker version identity in
the release/install path.

### Changed

- Split the image artifact track from the Firecracker version track so release
  metadata can distinguish guest image artifacts from the host VMM version.

### v0.2.3

Range: `v0.2.2..v0.2.3`

Human-reviewed by nathan on 2026-05-27

## [v0.2.3] — 2026-05-06

v0.2.3 fixes quickstart install layout after package extraction.

### Fixed

- Relocated the quickstart manifest after install so installed quickstart
  commands can find the manifest in the expected location.

### v0.2.4

Range: `v0.2.3..v0.2.4`

Human-reviewed by nathan on 2026-05-27

## [v0.2.4] — 2026-05-06

v0.2.4 repairs CI and smoke configuration for the release path.

### Fixed

- Fixed CI lint issues and smoke image configuration so release validation uses
  the intended image setup.

### v0.2.5

Range: `v0.2.4..v0.2.5`

Human-reviewed by nathan on 2026-05-27

## [v0.2.5] — 2026-05-06

v0.2.5 narrows the quickstart probe so it works without network egress.

### Fixed

- Ran the quickstart probe without requiring outbound network access, keeping
  first-run validation local to the installed release.

### v0.2.6

Range: `v0.2.5..v0.2.6`

Human-reviewed by nathan on 2026-05-27

## [v0.2.6] — 2026-05-06

v0.2.6 is a CI-quality repair release.

### Fixed

- Fixed clippy warnings that blocked the CI release path.

### v0.2.7

Range: `v0.2.6..v0.2.7`

Human-reviewed by nathan on 2026-05-27

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

### v0.2.8

Range: `v0.2.7..v0.2.8`

Human-reviewed by nathan on 2026-05-27

## [v0.2.8] — 2026-05-22

v0.2.8 completes current-latest repair proofing and preserves public installer
proof-cache material.

### Added

- Recorded public-access and current-latest publish proof artifacts for the
  release repair path.

### Changed

- Preserved proof-cache material from the public installer and made repair
  proofs policy-complete for current-latest validation.

### v0.2.9

Range: `v0.2.8..v0.2.9`

Human-reviewed by nathan on 2026-05-27

## [v0.2.9] — 2026-05-22

v0.2.9 repairs latest-promotion proof ordering.

### Fixed

- Proved the latest release state after promotion instead of before promotion,
  matching the public release channel's observable state.

### v0.2.10

Range: `v0.2.9..v0.2.10`

Human-reviewed by nathan on 2026-05-27

## [v0.2.10] — 2026-05-22

v0.2.10 fixes default release install selector placement.

### Fixed

- Used host selector state for default release installs so generated install
  metadata lands in the expected host location.

### v0.2.11

Range: `v0.2.10..v0.2.11`

Human-reviewed by nathan on 2026-05-27

## [v0.2.11] — 2026-05-22

v0.2.11 hardens generated VM identifier handling before launch.

### Fixed

- Added generated VM id checks before launch to catch path-budget issues before
  Firecracker setup mutates host state.

### v0.2.12

Range: `v0.2.11..v0.2.12`

Human-reviewed by nathan on 2026-05-27

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

### v0.2.13

Range: `v0.2.12..v0.2.13`

Human-reviewed by nathan on 2026-05-27

## [v0.2.13] — 2026-05-23

v0.2.13 repairs the release publish artifact handoff.

### Fixed

- Flattened the release artifact handoff so the publish job consumes the
  verified upload root rather than a nested artifact directory.

### v0.2.14

Range: `v0.2.13..v0.2.14`

Human-reviewed by nathan on 2026-05-27

## [v0.2.14] — 2026-05-23

v0.2.14 improves repository protection auditing for release tags.

### Fixed

- Hydrated release tag ruleset details before evaluating tag-protection
  readiness for release publication.

### v0.2.15

Range: `v0.2.14..v0.2.15`

Human-reviewed by nathan on 2026-05-27

## [v0.2.15] — 2026-05-23

v0.2.15 broadens acceptable release protection proof for the hosted workflow.

### Fixed

- Accepted an active branch ruleset with required status checks as the
  workflow-readable proof for protected release publication.

### v0.2.16

Range: `v0.2.15..v0.2.16`

Human-reviewed by nathan on 2026-05-27

## [v0.2.16] — 2026-05-23

v0.2.16 fixes release authority checks before draft creation.

### Fixed

- Treated a missing release as the expected pre-create state while preserving
  hard failures for unreadable release API metadata.

### v0.2.17

Range: `v0.2.16..v0.2.17`

Human-reviewed by nathan on 2026-05-27

## [v0.2.17] — 2026-05-23

v0.2.17 moves real-KVM proof out of the hosted latest-promotion gate.

### Changed

- Shifted real-KVM proof collection to closeout/freshness evidence so hosted
  latest promotion no longer depends on external real-KVM receipts.

### v0.2.18

Range: `v0.2.17..v0.2.18`

Human-reviewed by nathan on 2026-05-27

## [v0.2.18] — 2026-05-23

v0.2.18 preserves command lookup behavior inside the public installer.

### Fixed

- Preserved the public installer's configured command directory inside its
  sanitized child environment so installed command lookup works correctly.

### v0.2.19

Range: `v0.2.18..v0.2.19`

Human-reviewed by nathan on 2026-05-27

## [v0.2.19] — 2026-05-23

v0.2.19 retries transient lag in latest-release receipt generation.

### Fixed

- Added retry handling for transient stale-latest observations before declaring
  public-access receipt failure.

### v0.2.20

Range: `v0.2.19..v0.2.20`

Human-reviewed by nathan on 2026-05-27

## [v0.2.20] — 2026-05-23

v0.2.20 restores lockfile correctness while keeping the installer fixes from the
prior release line.

### Fixed

- Restored the lockfile after the v0.2.19 version bump accidentally rewrote the
  locked `libc` entry.

### v0.2.21

Range: `v0.2.20..v0.2.21`

Human-reviewed by nathan on 2026-05-27

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

### v0.2.22

Range: `v0.2.21..v0.2.22`

Human-reviewed by nathan on 2026-05-27

## [v0.2.22] — 2026-05-27

v0.2.22 fixes a legacy flat-cache installer backup case.

### Fixed

- Preserved legacy flat release-proof-cache content during install backup so old
  nested cache state no longer blocks activation.

### v0.2.23

Range: `v0.2.22..v0.2.23`

Human-reviewed by nathan on 2026-05-27

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

### v0.2.24

Range: `v0.2.23..v0.2.24`

Human-reviewed by nathan on 2026-05-27

## [v0.2.24] — 2026-05-27

v0.2.24 restores the complete flat binary projection for systemd-selected hosts.

### Fixed

- Always publish the flat `m80-jailer-harden` binary so default preflight and
  installer-owned host-binaries manifests agree on the stable `/opt/m80/bin`
  layout.

### v0.2.25

Range: `v0.2.24..v0.2.25`

Human-reviewed by nathan on 2026-05-27

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

## Next Action

This file is ready for human review. After review, the accepted sections should
be edited into `CHANGELOG.md` in reverse-chronological order, leaving
`[Unreleased]` with only post-backfill work. Follow
`docs/release/backfill-landing.md` for the review, verifier, and landing gate.
