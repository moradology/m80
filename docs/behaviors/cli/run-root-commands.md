# CLI Run-Root Commands

Behavior capture for bead `m80-lt15.6`.

## Contract

`m80 list` and `m80 inspect <vm-id>` are read-only views over persisted run-root
state. They do not contact a resident VM manager and do not require KVM.

The supported process-wrapper CLI deliberately has no `stop`, `exec`,
`launch`, or `snapshot` lifecycle command. Those old VM-manager surfaces are not
aliases for run-root walking.

## `m80 list`

`m80 list` enumerates subdirectories under the effective run-root and renders
each as:

- `live` when `ownership.lock` exists and records a `pid=` that is present under
  `/proc`.
- `stale` otherwise.

Persisted jailer files alone do not imply liveness; they remain after stop or
failure and are only evidence for inspection/recovery.

## `m80 inspect <vm-id>`

`m80 inspect` renders the selected run-dir, known lifecycle artifact names, and
JSON contents for known `.json` files when they are present:

- `ownership.lock`
- `jailer-state.json`
- `jailer-plan.json`
- `cgroup-path.txt`
- `boot-identity.json`

`boot-identity.json` is reserved for snapshot/restore identity and may be absent
in v0.1.

## Cleanup Boundary

`m80 cleanup` remains a supported admin command, but it is backend/preflight
backed and is not a hidden stop implementation. Non-KVM tests for cleanup need a
separate recovery seam; this behavior capture only covers read-only run-root
views.

## Tests

- unit tests in `crates/m80-cli/src/cmds_walk.rs`
- `crates/m80-cli/tests/parse_args.rs`
- `crates/m80-cli/tests/help_smoke.rs`
