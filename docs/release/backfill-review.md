# v0.2 Backfill Review Packet

Status: draft suggestions only. Human review/refinement is still required before
any section below lands in `CHANGELOG.md`.

Generated: 2026-05-27

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
| v0.2.0 | 2026-05-06 | v0.1.0-smoke-passing..v0.2.0 | 65 | pending human review |
| v0.2.1 | 2026-05-06 | v0.2.0..v0.2.1 | 1 | pending human review |
| v0.2.2 | 2026-05-06 | v0.2.1..v0.2.2 | 1 | pending human review |
| v0.2.3 | 2026-05-06 | v0.2.2..v0.2.3 | 1 | pending human review |
| v0.2.4 | 2026-05-06 | v0.2.3..v0.2.4 | 1 | pending human review |
| v0.2.5 | 2026-05-06 | v0.2.4..v0.2.5 | 1 | pending human review |
| v0.2.6 | 2026-05-06 | v0.2.5..v0.2.6 | 1 | pending human review |
| v0.2.7 | 2026-05-22 | v0.2.6..v0.2.7 | 755 | pending human review |
| v0.2.8 | 2026-05-22 | v0.2.7..v0.2.8 | 9 | pending human review |
| v0.2.9 | 2026-05-22 | v0.2.8..v0.2.9 | 1 | pending human review |
| v0.2.10 | 2026-05-22 | v0.2.9..v0.2.10 | 1 | pending human review |
| v0.2.11 | 2026-05-22 | v0.2.10..v0.2.11 | 1 | pending human review |
| v0.2.12 | 2026-05-23 | v0.2.11..v0.2.12 | 224 | pending human review |
| v0.2.13 | 2026-05-23 | v0.2.12..v0.2.13 | 1 | pending human review |
| v0.2.14 | 2026-05-23 | v0.2.13..v0.2.14 | 1 | pending human review |
| v0.2.15 | 2026-05-23 | v0.2.14..v0.2.15 | 1 | pending human review |
| v0.2.16 | 2026-05-23 | v0.2.15..v0.2.16 | 1 | pending human review |
| v0.2.17 | 2026-05-23 | v0.2.16..v0.2.17 | 1 | pending human review |
| v0.2.18 | 2026-05-23 | v0.2.17..v0.2.18 | 1 | pending human review |
| v0.2.19 | 2026-05-23 | v0.2.18..v0.2.19 | 1 | pending human review |
| v0.2.20 | 2026-05-23 | v0.2.19..v0.2.20 | 1 | pending human review |
| v0.2.21 | 2026-05-27 | v0.2.20..v0.2.21 | 101 | pending human review |
| v0.2.22 | 2026-05-27 | v0.2.21..v0.2.22 | 1 | pending human review |
| v0.2.23 | 2026-05-27 | v0.2.22..v0.2.23 | 31 | pending human review |
| v0.2.24 | 2026-05-27 | v0.2.23..v0.2.24 | 1 | pending human review |
| v0.2.25 | 2026-05-27 | v0.2.24..v0.2.25 | 2 | pending human review |

## Draft Sections

### v0.2.0

Range: `v0.1.0-smoke-passing..v0.2.0`

Draft suggestion — human review required

## [v0.2.0] — 2026-05-06

v0.2.0 turns the smoke-passing prototype into the first process-wrapper release:
the CLI can run commands as constrained Firecracker processes, the image stack
supports minimal boot images, and the core lifecycle gained the performance
primitives used by later releases.

### Added

- Added the `m80 run` process-wrapper product surface, including workspace,
  environment, stdout/stderr, exit-code, and first-run quickstart behavior.
- Added minimal-image build and manifest support, PID-1 guestd boot, boot-args
  dispatch by image/kernel kind, and smoke coverage for the minimal image path.
- Added snapshot capture/restore plumbing, a persistent execution path, idle
  timeout handling, cancellation, warm-pool groundwork, and per-phase timing
  instrumentation for launch-latency work.
- Added storage pivot groundwork: sparse per-VM overlays, host/guest drive
  layout changes, and guest overlay mount/pivot behavior.
- Added vsock graceful-stop and inverted-readiness fixes to make shutdown and
  boot readiness deterministic.

### Changed

- Reworked the release and docs surface around the process-wrapper model rather
  than exposing Firecracker mechanics as the primary user workflow.
- Tightened ready-probe cadence and removed the earlier smoke retry loop.

### Fixed

- Fixed the systemd guestd boot ordering cycle and converted readiness to the
  host-observable vsock path.

### v0.2.1

Range: `v0.2.0..v0.2.1`

Draft suggestion — human review required

## [v0.2.1] — 2026-05-06

v0.2.1 repairs the release workflow after the initial v0.2.0 cut.

### Fixed

- Fixed the release workflow's musl target installation path so release
  artifact builds can install the expected target toolchain.

### v0.2.2

Range: `v0.2.1..v0.2.2`

Draft suggestion — human review required

## [v0.2.2] — 2026-05-06

v0.2.2 separates image artifact identity from Firecracker version identity in
the release/install path.

### Changed

- Split the image artifact track from the Firecracker version track so release
  metadata can distinguish guest image artifacts from the host VMM version.

### v0.2.3

Range: `v0.2.2..v0.2.3`

Draft suggestion — human review required

## [v0.2.3] — 2026-05-06

v0.2.3 fixes quickstart install layout after package extraction.

### Fixed

- Relocated the quickstart manifest after install so installed quickstart
  commands can find the manifest in the expected location.

### v0.2.4

Range: `v0.2.3..v0.2.4`

Draft suggestion — human review required

## [v0.2.4] — 2026-05-06

v0.2.4 repairs CI and smoke configuration for the release path.

### Fixed

- Fixed CI lint issues and smoke image configuration so release validation uses
  the intended image setup.

### v0.2.5

Range: `v0.2.4..v0.2.5`

Draft suggestion — human review required

## [v0.2.5] — 2026-05-06

v0.2.5 narrows the quickstart probe so it works without network egress.

### Fixed

- Ran the quickstart probe without requiring outbound network access, keeping
  first-run validation local to the installed release.

### v0.2.6

Range: `v0.2.5..v0.2.6`

Draft suggestion — human review required

## [v0.2.6] — 2026-05-06

v0.2.6 is a CI-quality repair release.

### Fixed

- Fixed clippy warnings that blocked the CI release path.

### v0.2.7

Range: `v0.2.6..v0.2.7`

Draft suggestion — human review required

## [v0.2.7] — 2026-05-22

v0.2.7 is a large hardening and release-install epoch. It replaces the active
wire protocol, expands adversarial coverage, improves jailer/guest isolation,
and adds the release machinery used to prove public install artifacts.

### Added

- Added protobuf length-prefixed framing for the host/guest wire protocol,
  replacing the previous JSON/base64 payload path.
- Added broad real-KVM and fixture coverage for file operations, streaming,
  PTY, cancellation, idle timeout, warm pool, snapshot restore, protocol
  version rejection, diagnostics, cgroup behavior, storage overlays, outbound
  networking, jailer recovery, and malicious guestd scenarios.
- Added OutboundNat and JoinNetns networking surfaces, Firecracker drive PATCH
  and hotplug contracts, guest uevent wait handling, and verified drive attach
  workflow support.
- Added guestd hardening improvements, including host liveness detection,
  Ping/Pong health, CPU/memory hotplug event handling, and PID-1 storage repair
  fallback.
- Added jailer/security parity work covering cgroup setup, jail recovery,
  daemonized launch, Bestiary conveyor-belt primitives, and attack-runner
  harnesses.
- Added release/public-install proof machinery for current-latest repair,
  freshness policy, workflow policy checks, public access receipts, and release
  asset verification.

### Changed

- Hard-cut the wire protocol to protobuf framing with no active NDJSON fallback.
- Simplified and normalized crate APIs, READMEs, and test helpers through
  multi-round trim/refactor work.

### Fixed

- Fixed multiple launch, cancellation, cleanup, storage, network, cgroup, and
  release-publication regressions found while expanding the hardening/test
  surface.

### v0.2.8

Range: `v0.2.7..v0.2.8`

Draft suggestion — human review required

## [v0.2.8] — 2026-05-22

v0.2.8 completes the current-latest repair proof and preserves public installer
proof-cache material.

### Added

- Recorded public-access and current-latest publish proof artifacts for the
  release repair path.

### Changed

- Preserved proof-cache material from the public installer and made repair
  proofs policy-complete for current-latest validation.

### v0.2.9

Range: `v0.2.8..v0.2.9`

Draft suggestion — human review required

## [v0.2.9] — 2026-05-22

v0.2.9 repairs latest-promotion proof ordering.

### Fixed

- Proved the latest release state after promotion instead of before promotion,
  matching the public release channel's observable state.

### v0.2.10

Range: `v0.2.9..v0.2.10`

Draft suggestion — human review required

## [v0.2.10] — 2026-05-22

v0.2.10 fixes default release install selector placement.

### Fixed

- Used host selector state for default release installs so generated install
  metadata lands in the expected host location.

### v0.2.11

Range: `v0.2.10..v0.2.11`

Draft suggestion — human review required

## [v0.2.11] — 2026-05-22

v0.2.11 hardens generated VM identifier handling before launch.

### Fixed

- Added generated VM id checks before launch to catch path-budget issues before
  Firecracker setup mutates host state.

### v0.2.12

Range: `v0.2.11..v0.2.12`

Draft suggestion — human review required

## [v0.2.12] — 2026-05-23

v0.2.12 consolidates the release-install epoch and adds operator support
artifacts around freshness, bug reporting, proof ledgers, and install
transaction validation.

### Added

- Added the redacted `m80 bug-report` support bundle and related operator
  diagnostic material.
- Added release proof asset emission, proof-ledger validation, public-access
  receipt handling, install-handoff verification, release readiness receipts,
  and freshness/status renderer coverage.
- Added troubleshooting coverage for quickstart and release-install failure
  modes.

### Changed

- Consolidated release install epoch tracker state and command inventories.
- Hardened install transaction proof handling and freshness command digest
  generation.

### Fixed

- Repaired release proof citations and generated documentation assertions found
  while closing the release-install proof graph.

### v0.2.13

Range: `v0.2.12..v0.2.13`

Draft suggestion — human review required

## [v0.2.13] — 2026-05-23

v0.2.13 repairs the release publish artifact handoff.

### Fixed

- Flattened the release artifact handoff so the publish job consumes the
  verified upload root rather than a nested artifact directory.

### v0.2.14

Range: `v0.2.13..v0.2.14`

Draft suggestion — human review required

## [v0.2.14] — 2026-05-23

v0.2.14 improves repository protection auditing for release tags.

### Fixed

- Hydrated release tag ruleset details before evaluating tag-protection
  readiness for release publication.

### v0.2.15

Range: `v0.2.14..v0.2.15`

Draft suggestion — human review required

## [v0.2.15] — 2026-05-23

v0.2.15 broadens acceptable release protection proof for the hosted workflow.

### Fixed

- Accepted an active branch ruleset with required status checks as the
  workflow-readable proof for protected release publication.

### v0.2.16

Range: `v0.2.15..v0.2.16`

Draft suggestion — human review required

## [v0.2.16] — 2026-05-23

v0.2.16 fixes release authority checks before draft creation.

### Fixed

- Treated a missing release as the expected pre-create state while preserving
  hard failures for unreadable release API metadata.

### v0.2.17

Range: `v0.2.16..v0.2.17`

Draft suggestion — human review required

## [v0.2.17] — 2026-05-23

v0.2.17 moves real-KVM proof out of the hosted latest-promotion gate.

### Changed

- Shifted real-KVM proof collection to closeout/freshness evidence so hosted
  latest promotion no longer depends on external real-KVM receipts.

### v0.2.18

Range: `v0.2.17..v0.2.18`

Draft suggestion — human review required

## [v0.2.18] — 2026-05-23

v0.2.18 preserves command lookup behavior inside the public installer.

### Fixed

- Preserved the public installer's configured command directory inside its
  sanitized child environment so installed command lookup works correctly.

### v0.2.19

Range: `v0.2.18..v0.2.19`

Draft suggestion — human review required

## [v0.2.19] — 2026-05-23

v0.2.19 retries transient lag in latest-release receipt generation.

### Fixed

- Added retry handling for transient stale-latest observations before declaring
  public-access receipt failure.

### v0.2.20

Range: `v0.2.19..v0.2.20`

Draft suggestion — human review required

## [v0.2.20] — 2026-05-23

v0.2.20 restores lockfile correctness while keeping the installer fixes from the
prior release line.

### Fixed

- Restored the lockfile after the v0.2.19 version bump accidentally rewrote the
  locked `libc` entry.

### v0.2.21

Range: `v0.2.20..v0.2.21`

Draft suggestion — human review required

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

Draft suggestion — human review required

## [v0.2.22] — 2026-05-27

v0.2.22 fixes a legacy flat-cache installer backup case.

### Fixed

- Preserved legacy flat release-proof-cache content during install backup so old
  nested cache state no longer blocks activation.

### v0.2.23

Range: `v0.2.22..v0.2.23`

Draft suggestion — human review required

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

Draft suggestion — human review required

## [v0.2.24] — 2026-05-27

v0.2.24 restores the complete flat binary projection for systemd-selected hosts.

### Fixed

- Always publish the flat `m80-jailer-harden` binary so default preflight and
  installer-owned host-binaries manifests agree on the stable `/opt/m80/bin`
  layout.

### v0.2.25

Range: `v0.2.24..v0.2.25`

Draft suggestion — human review required

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
`[Unreleased]` with only post-backfill work.
