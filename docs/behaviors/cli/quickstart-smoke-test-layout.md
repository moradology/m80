# Quickstart Smoke Test Layout

The `m80 quickstart --no-run` integration tests are grouped by failure class so
the first failing behavior points at the relevant contract instead of a
monolithic smoke file.

- `crates/m80-cli/tests/quickstart_smoke/artifact_integrity.rs` covers tarball
  checksum admission and installed immutable artifact verification.
- `crates/m80-cli/tests/quickstart_smoke/installed_profile.rs` covers default
  profile and config writes, JSON output, release-tag capture, and manifest
  kernel-kind projection.
- `crates/m80-cli/tests/quickstart_smoke/rollback.rs` covers failed config
  writes and invalid existing config state without tamper-and-restore fixtures.
- `crates/m80-cli/tests/quickstart_smoke/host_manifest.rs` covers
  host-binaries manifest semantics and rejects bundled operator-owned host
  prerequisites.
- `crates/m80-cli/tests/quickstart_smoke/probe.rs` covers probe gating behavior
  that must fail before active install paths are created.

Shared release-tarball and profile fixtures live in
`crates/m80-cli/tests/quickstart_smoke/support.rs`. Each failure scenario stays
as its own `#[test]`; do not collapse them into table cases that mask later
failures after the first assertion trips.
