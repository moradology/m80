# Teardown Phase Order

## Phase Sequence

`m80-firecracker` exposes cleanup as an ordered VM-mechanics sequence:
`admission_fence -> bounded_stop -> optional_change_extract -> residue_cleanup
-> release`.

In v0.1, the admission fence is the `RunningSandbox` type-state boundary:
`stop` and `force_kill` consume the running handle, so no later exec can be
submitted through that handle. `bounded_stop` is guestd shutdown plus host kill
for `stop`, or immediate host kill for `force_kill`. Change extraction is not
automatic; callers may invoke `StoppedSandbox::extract_changes` before release.
Residue cleanup is owned by foundation drops, stale run-root recovery, and
`m80-net-outbound` for outbound NAT residue. Release occurs when the stopped
handle is consumed by `delete` or `preserve_for_triage`.

This is the m80 cutover from the older predecessor single `force_stop_and_cleanup`
routine: the ordering is preserved, but the implementation is split across
typed lifecycle transitions instead of one placement-aware cleanup function.

Tests:
- `crates/m80-firecracker/tests/cleanup/teardown_phase_order.rs::phases_run_in_documented_order`
- `crates/m80-firecracker/tests/cleanup/teardown_phase_order.rs::admission_fence_precedes_destructive_cleanup`

## Admission Fence

The fence is local to the sandbox handle. Once teardown starts, the caller has
moved the `RunningSandbox` into `stop` or `force_kill`; Rust then prevents
another exec through that handle. m80 does not own any higher-level queue or
placement lease, so it does not claim to fence work outside the handle it owns.

Test:
- `crates/m80-firecracker/tests/cleanup/teardown_phase_order.rs::admission_fence_precedes_destructive_cleanup`
