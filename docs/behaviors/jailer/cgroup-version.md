# Jailer Cgroup Version

## Behavior

m80 uses cgroup v2 for VM resource containment. Every `m80-firecracker` cold or
restored launch therefore builds a `JailerConfig` with
`cgroup_version = Some(CgroupVersion::V2)`, and `m80-jailer` emits
`--cgroup-version 2` before the Firecracker-side `--` separator when it execs
the official Firecracker jailer.

`JailerConfig::cgroup_version = None` and `Some(CgroupVersion::V1)` leave the
official jailer argument absent. That preserves the low-level crate's ability to
represent the official jailer's native surface, while the m80 orchestrator's
actual launch path hard-cuts to v2.

## Tests

- `crates/m80-jailer/src/materialized_tests.rs` records fake-jailer argv and
  asserts `V2` emits `--cgroup-version 2`, while `None` and `V1` omit it.
- `crates/m80-jailer/tests/jailer/cgroup_version.rs` asserts `V2` persists in
  the replayable plan JSON.
- `crates/m80-firecracker/src/launch/tests.rs` asserts the orchestrator-built
  `JailerConfig` selects `Some(CgroupVersion::V2)`.
