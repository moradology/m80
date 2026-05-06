# CLI Writeback Policy

Behavior capture for bead `m80-lt15.17.2`.

## Scope

`--writeback` controls workspace scratch extraction only. It never commits rootfs
overlay writes back into the selected runtime image/profile.

The policy requires `--workspace`; writeback with no workspace fails as a
wrapper configuration error before backend/preflight work starts.

Verification:
`crates/m80-cli/tests/output_error_contract.rs::writeback_without_workspace_fails_before_backend_work`.

## Never

`--writeback never` is the default. The guest may write to `/workspace` during
the run, but the host workspace is not updated by the CLI after stop/delete.

Verification:
`crates/m80-cli/tests/parse_args.rs::parse_run_defaults_to_process_wrapper_contract`.

## On Success

`--writeback on-success` extracts workspace changes only when the guest process
exits with code `0`. Non-zero guest exits preserve the guest exit code and do
not extract changes.

Verification:
`crates/m80-cli/src/cmds/tests.rs::writeback_policy_decides_from_guest_exit`.

## Always

`--writeback always` extracts workspace changes after guest process completion
for both zero and non-zero guest exits. If extraction succeeds, the CLI still
returns the original guest exit code.

Verification:
`crates/m80-cli/src/cmds/tests.rs::writeback_policy_decides_from_guest_exit`.

## Extraction Failure

Workspace extraction failure is a wrapper failure, not a child exit code. The
CLI attempts to preserve the stopped run directory for triage and then returns
the typed wrapper error. It does not delete ambiguous workspace state and does
not pretend the child exit code describes the writeback result.

## Conflict And Admissibility

The lower storage layer owns workspace extraction admissibility. Symlinks,
special files, and other unsafe extracted entries are rejected there. Those
failures surface through the wrapper error path above.

## Non-Goals

There is no agent `EffectClass`, authority lease, commit deadline, or tool
registry policy in this writeback surface. It is a generic process-wrapper
effect knob: copy workspace scratch changes back to the host according to a
named policy, or fail closed.
