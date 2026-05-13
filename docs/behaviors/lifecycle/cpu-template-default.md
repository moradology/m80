# Behavior: CPU Template Default

**Bead:** `m80-jp6ik.27`
**Date:** 2026-05-13
**Test:** `crates/m80-firecracker/src/preboot_tests.rs`

## Contract

`SandboxConfig::cpu_template` defaults to `None`. The default preboot
`PUT /machine-config` serializes no `cpu_template` field and does not inspect
`/proc/cpuinfo` to auto-select T2 on Intel hosts.

Callers that need Firecracker's AWS template-masked CPU surface opt in by
setting `SandboxConfig::cpu_template` to `CpuTemplate::T2` or
`CpuTemplate::C3`. That keeps the default path optimized for same-host launch
and restore while preserving an explicit portability knob.

## Snapshot Tradeoff

m80's default snapshot contract is same-host restore. Future cross-host restore
support must verify host CPU-feature parity before accepting a restore target;
it must not depend on silent template masking as a substitute for compatibility
admission.
