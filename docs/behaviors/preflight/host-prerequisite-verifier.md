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

- `check_id`, the stable machine identity;
- `check_name`;
- optional final path;
- optional expected/actual scalar values;
- optional expected/actual version, sha256, mode, and owner facts;
- pass/fail status;
- a typed `failure_variant` for failed checks;
- a remediation id plus either an exact command or a policy/runbook link.

Machine consumers key on `check_id`, not `check_name`. Human labels may change
for table readability without changing the machine identity. The registry is:

`os_gate`, `host_kernel_floor`, `kvm`, `cgroup_mode`, `jailer_identity`,
`privilege`, `host_substrate_proof`, `kvm_cpu_extensions`, `kernel_modules`,
`transparent_hugepages`, `kvm_halt_polling`, `cpu_governor`,
`cpu_microcode`, `cpu_vulnerabilities`, `conntrack_capacity`,
`firecracker_binary`, `firecracker_seccomp_filter`, `jailer_binary`,
`jailer_hardening_wrapper`, `network_helper`, `host_binary_manifest`,
`kernel_image`, `rootfs_manifest`, `run_root`, `run_root_filesystem`, and
`storage_helpers`.

Production machine consumers are linted against decisions keyed on
`CheckRow.label` or `HostPrerequisiteCheck.check_name`. Human renderers may use
those fields only for display, and any necessary exception must carry the narrow
`m80-check-id-lint: allow-human-label-renderer` marker next to the display-only
code. Installers, release proof readers, freshness/status checks, diagnostics,
and other machine decisions use `HostPrerequisiteCheckId` or serialized
`check_id` values.

Readers fail closed on unknown schema versions, missing required fields,
unknown check ids, unknown failure variants, and failed checks without a
remediation token. The host substrate verifier emits this result for its rows
today. `HostPrerequisiteCheck::from_preflight_error()` projects typed preflight
failures into the same schema, including final path, expected/actual scalar
values, release version fields when known, a typed failure variant, and a
policy-linked remediation token. `m80 preflight` renders that projected record
in both JSON and plain text so machine consumers and operators see the same
structured repair fields. When the selected runtime profile names a release
tag, CLI rendering pins m80-owned helper repairs to that exact `install.sh`
URL; operator-owned Firecracker, jailer, and seccomp repairs remain policy
links. Full host-binary path/hash/mode population is owned by
`m80-o3uh9.8.3.2`.

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
