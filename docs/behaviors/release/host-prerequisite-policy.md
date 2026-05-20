# Host Prerequisite Policy

m80 v0.x treats Firecracker, the official jailer, and Firecracker's seccomp
filter as operator-provided host prerequisites. The m80 release bundle owns the
m80 binary, m80 host helpers, guest artifacts, manifests, receipts, installer,
and checksum material. It does not bundle or install the official Firecracker
VMM, the official jailer, or the Firecracker seccomp filter.

This is a security and operations boundary. Firecracker and the jailer are
privileged host TCB components with their own CVE and fleet-patching cadence.
m80 must fail before active install changes when those host prerequisites are
missing, stale, or unsupported; it must not silently replace them from an m80
bundle.

## Supported Mode

The only supported v0.x mode is operator-provided Firecracker:

- `/opt/firecracker/bin/firecracker` by default, or
  `M80_FIRECRACKER_BIN`;
- `/opt/firecracker/bin/jailer` by default, or `M80_JAILER_BIN`;
- `/opt/firecracker/bin/firecracker-seccomp-filter.bin` by default, or
  `M80_FIRECRACKER_SECCOMP_FILTER`.

Bundled Firecracker mode is unsupported for v0.x. A release bundle that carries
official Firecracker, official jailer, or the Firecracker seccomp filter is not
a valid m80 release bundle. If m80 ever starts installing pinned Firecracker
bytes itself, that is a new policy decision and a hard cutover, not a tolerated
compatibility mode.

The fail-closed checks are explicit. `scripts/package-release-bundle.py` has no
inputs or payload destinations for official Firecracker, official jailer, or
Firecracker seccomp filter bytes. `scripts/verify-release-bundle.py` rejects
tar entries such as `bin/firecracker`, `bin/jailer`, and
`bin/firecracker-seccomp-filter.bin` with this policy named in the error.
`m80 quickstart` rejects the same payload names before creating or changing the
active artifact directory.

## Version Sources

The expected Firecracker train for a guest artifact set comes from the verified
guest manifest field `expected_firecracker_version`. Release packaging mirrors
that value into `bundle.json` and rejects bundle/manifest disagreement. Runtime
preflight compares the installed `firecracker --version` output to the verified
guest manifest value before launch. `M80_FIRECRACKER_VERSION` is an optional
pre-artifact pin for installer and operator verification, but it is not a
replacement for the manifest check.

The jailer version source of truth is the accepted Firecracker train: the
official jailer must come from the same Firecracker release train and exact
release as the accepted Firecracker binary. The train policy source is
`crates/m80-preflight/src/firecracker_train.rs`; its
`FirecrackerTrainPolicy::from_expected_firecracker_version` returns the
expected Firecracker version, the official jailer pairing rule, and the active
CVE-floor table used by preflight and installer/release verification. Preflight
probes both `firecracker --version` and `jailer --version`; malformed output or
version disagreement returns a typed failure before launch. The installed-byte
identity check remains `host-binaries.manifest.json`; that manifest records the
Firecracker seccomp filter as launch material rather than as a host binary, and
records the Firecracker train that seccomp filter is expected to match.

Firecracker CVE floors live in `crates/m80-preflight/src/cve_floor.rs` and are
documented in `docs/security/firecracker-cve-floor.md`. That code is the
runtime source of truth for rejecting known-vulnerable Firecracker versions.

## Host Substrate

Install and launch assume a Linux/KVM host with:

- `/dev/kvm` present and writable by the run identity;
- cgroup v2 available when `M80_CGROUP_MODE=unified-v2`;
- configured jail UID/GID present and non-root;
- startup privilege through root, the documented m80 capability set, or a
  privileged container that grants that set;
- root-owned, non-writable host TCB paths for Firecracker, jailer, m80 helpers,
  guest artifacts, and `host-binaries.manifest.json`.

The installer may run as root or through sudo for privileged copies and active
install finalization. Runtime launch may run as root or as a capability-bearing
process admitted by `m80-preflight`. m80 does not auto-create host identities,
load kernel modules, rewrite cgroup mode, or repair `/dev/kvm` permissions.

## Failure Contract

Host-prerequisite failures point back to this policy and to the concrete repair
document:

- `docs/ops/host-setup.md` for operator host posture;
- `docs/ops/binary-installation.md` for final installed paths, ownership, and
  host-binaries manifest generation;
- `docs/behaviors/release/host-prerequisite-fixtures.md` for the hostless
  install-root fixture matrix;
- `docs/behaviors/preflight/host-prerequisite-verifier.md` for the reusable
  non-mutating substrate verifier used by install and preflight;
- `docs/behaviors/preflight/binary-discovery.md` for current preflight
  discovery and identity checks.

Release proof artifacts should include a saved `m80 preflight --json` payload.
Its `data.runtime_profile` names the selected profile, active install pointer,
artifact/helper paths, and release tag. Its `data.host_prerequisites` is the
schema-versioned `HostPrerequisiteResult`: the `Firecracker binary` check
records the expected and observed Firecracker version, and the `Jailer binary`
check records the expected jailer version and the observed jailer version.
Those checks are the host-local proof that the operator-provided train matches
the guest artifact set.

Failed `m80 preflight` output includes `data.host_prerequisite_failure`, a
single failed `HostPrerequisiteCheck` carrying the same check id,
expected/actual value fields, final path, failure variant, and remediation
policy link that plain text renders for operators.

The user-facing rule is short: install the m80 bundle, provide the official
Firecracker host prerequisites at their configured paths, run `m80 preflight`,
then run `m80 run -- echo hello`.
