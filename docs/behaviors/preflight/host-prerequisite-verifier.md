# Host Prerequisite Verifier

`m80-preflight::verify_host_substrate()` is the non-mutating host-substrate
verifier used before install finalization and by the full launch preflight.
`verify_host_substrate_fixture()` exposes the same classifier through a
hostless `HostSubstrateFixture`. Neither path downloads, chmods, creates users,
loads modules, rewrites cgroups, or repairs `/dev/kvm`; they only observe the
chosen substrate and return typed failures with the same `PreflightError::hint()`
remediation text that the CLI renders.

## Checked Fields

The verifier checks:

- `uname -s` is `Linux`;
- `uname -r` parses as Linux kernel `6.1` or newer;
- `/dev/kvm` exists and opens for write by the current process;
- `cgroup_mode = "unified-v2"` passes `m80-cgroup::Subtree::probe()`, while
  `cgroup_mode = "disabled"` explicitly skips the cgroup v2 probe;
- configured `jail_uid` and `jail_gid` resolve in the host passwd/group
  databases and do not fall back to another identity;
- the process is either `euid == 0` or has every capability in
  `REQUIRED_CAPABILITIES`.

Each failure is typed: unsupported host, old kernel, missing or non-writable
KVM, unavailable cgroup v2, missing jail identity, and insufficient privilege
all have distinct `PreflightError` variants. The human repair text comes from
the same `hint()` mapping used by `m80 preflight`.

## Result Contract

Host prerequisite proof uses `HostPrerequisiteResult` with
`HOST_PREREQUISITE_RESULT_SCHEMA_VERSION`. The result records ordered checks in
a schema-versioned structure instead of asking installers, diagnostics, release
proofs, or docs tests to scrape table rows. Each check has:

- `check_name`;
- optional final path;
- optional expected/actual version, sha256, mode, and owner facts;
- pass/fail status;
- a typed `failure_variant` for failed checks;
- a remediation id plus either an exact command or a policy/runbook link.

Readers fail closed on unknown schema versions, missing required fields,
unknown failure variants, and failed checks without a remediation token. The
host substrate verifier emits this result for its rows today. Full host-binary
path/hash/mode population is owned by `m80-o3uh9.8.3.2`; diagnostics rendering
of these fields is owned by `m80-o3uh9.8.3.3`.

## Fixture Knobs

`HostSubstrateFixture` can simulate the substrate without touching `/opt`,
`/etc`, real `/dev/kvm`, system cgroups, host passwd/group databases, or real
capability state. Fixture knobs cover host platform, kernel release,
`HostSubstrateFixtureKvm::{Writable, Missing, NotWritable}`, cgroup v2
availability, jail user/group lookup, effective uid, and effective capability
set.

A successful hostless fixture emits `HostSubstrateProofKind::HostlessFixture`
and a `Host substrate proof` row containing `hostless fixture only; not a
real-KVM run-smoke proof`. A live `m80 preflight` emits
`HostSubstrateProofKind::LivePreflight`; that also is not a substitute for the
release `m80 run -- echo hello` smoke.

## Evidence

- `crates/m80-preflight/src/substrate.rs`
- `crates/m80-preflight/src/host_prerequisite_result.rs`
- `crates/m80-preflight/src/checks.rs`
- `crates/m80-preflight/tests/host_prerequisite_result.rs`
- `crates/m80-preflight/src/substrate.rs::tests::hostless_fixture_success_is_not_real_kvm_smoke`
- `crates/m80-preflight/src/substrate.rs::tests::substrate_fixture_rejects_missing_kvm`
- `crates/m80-preflight/src/substrate.rs::tests::substrate_fixture_rejects_bad_kvm_permissions`
- `crates/m80-preflight/src/substrate.rs::tests::substrate_fixture_rejects_wrong_cgroup_mode`
- `crates/m80-preflight/src/substrate.rs::tests::substrate_fixture_rejects_insufficient_privilege`
- `crates/m80-preflight/tests/host_prerequisite_fixtures.rs`
