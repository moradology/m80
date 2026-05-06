# CLI Warm Lifetime

Behavior capture for bead `m80-lt15.9`.

## Current Status

`m80 warm` has a concrete foreground owner mode. `m80 warm enable --foreground`
starts a visible long-running owner process, creates a clean snapshot, fills a
`WarmPool`, and listens on a Unix control socket under the warm run-root.
`m80 warm status`, `drain`, `disable`, and `m80 run --warm` talk to that owner.
No hidden warm owner, pool daemon, or cold fallback exists behind `run --warm`.
`m80 warm enable --system` remains a feature gap until service packaging lands.

Verification:
`crates/m80-cli/tests/feature_gap_smoke.rs::warm_status_without_owner_is_unavailable_without_preflight`,
`crates/m80-cli/tests/feature_gap_smoke.rs::run_warm_without_owner_fails_without_cold_booting`,
`crates/m80-cli/src/cmds/warm/run.rs::tests::run_request_empty_pool_returns_pool_empty_without_cold_boot`,
and `crates/m80-cli/tests/e2e_warm.rs::foreground_warm_owner_serves_run_and_drains_without_cold_fallback`.

## What A Warm VM Is Doing

A warm VM is a clean, stateless, pre-restored slot owned by an explicit resident
warm owner. While idle, it has:

- restored from a clean snapshot
- passed the configured guestd ready probe
- guestd listening for one future exec
- no user process running
- no workspace attached
- no run id or request id attached
- no caller secrets projected
- no terminal attached

The slot is waiting to be leased for exactly one process run. It is not a user
session, not a shell, not a hidden Docker-like container, and not a place where
callers can accumulate state between runs.

The lower lifecycle contract is `WarmPool`: ready slots are leased through
`WarmLease`; empty pools return `FcError::PoolEmpty`; leased slots are discarded
unless complete reset evidence proves reuse is safe.

Reference:
`docs/behaviors/cli/warm-owner-lifecycle.md`,
`docs/behaviors/lifecycle/warm-pool.md`, and `docs/design/warm-pool.md`.

## CLI Shape

The user-facing shape is:

```text
m80 warm enable --foreground --size <n> [--profile <name>] [--egress none|outbound]
m80 warm status [--profile <name>]
m80 warm drain
m80 warm disable
m80 run --warm -- <program> [args...]
```

`enable --foreground` starts the explicit resident owner and fills it to the
target ready-slot count before listening for leases. `status` reports owner
identity, configured profile, ready/filling/leased counts, and the last fill
error. `drain` stops accepting new leases and tears down ready/filling slots as
the foreground owner exits. `disable` asks the owner to stop and removes
owner-owned socket/identity state; when no owner is running it performs that
cleanup directly.

There is deliberately no `m80 pool` command in the process-wrapper facade. The
user-facing concept is warm execution, not general VM pool management.

## Resident Owner

The owner is visible and intentionally enabled. Foreground mode is implemented
first; system service packaging is reserved behind
`m80 warm enable --system`, and user-service mode is deferred. The owner
identity is explicit in `status` output and diagnostics.

`m80 run --warm` talks to that owner. If the owner is unavailable, the command
fails explicitly with a typed wrapper error. If the owner is available but no
ready slot exists, the command returns the warm-empty path (`FcError::PoolEmpty`,
CLI exit 8). It never cold-boots under a command that requested warm capacity.
For non-JSON runs, stdout/stderr are streamed over the owner socket as guestd
emits chunks; the terminal frame supplies the child exit code. JSON warm runs
remain buffered so the response envelope is a single object.
The client-generated `request_id` is part of the warm control request. The owner
uses it for the leased slot's exec wire frames and returns it in warm run/error
responses.

## Eligibility

Warm slots are eligible only for clean stateless runs. The warm profile identity
must include every input that affects whether a ready slot is valid:

- selected CLI profile name
- kernel image sha
- rootfs/base image sha
- manifest schema and image kind
- overlay template identity
- Firecracker version pin
- egress policy
- vCPU and memory sizing
- no-workspace status
- guestd ready-probe request

`m80 run --warm --workspace ...` is incompatible until a separate attach-late
workspace design lands. The correct behavior is a typed failure, not cold
fallback and not silent loss of the workspace.

## Upgrade And Crash Behavior

Binary, artifact, manifest, profile, sizing, or egress-policy changes invalidate
existing ready slots. The owner must drain/delete mismatched slots and refill
from the new identity before leasing again.

If the resident owner crashes, all ready/filling slot state is treated as lost
until a new owner performs explicit recovery. A warm run during that interval
fails as owner-unavailable; it does not probe old sockets and infer that a slot
is reusable.

## Idle And Drain

Warm slots are lifetime-owned by the resident owner. Per-slot idle shutdown must
be disabled or handled by the owner so a Ready slot does not silently expire
behind the owner's accounting. If a slot does expire or fails a health probe,
the owner discards it and refills.

Drain is explicit. It prevents new leases, lets existing leases complete or be
discarded by policy, deletes ready/filling slots, and reports completion only
after owned host residue is gone.

## Tests

The CLI and lifecycle tests pin both the foreground owner and lower-level
allocation behavior:

- `crates/m80-cli/tests/parse_args.rs::parse_warm_enable_foreground_shape`
- `crates/m80-cli/tests/feature_gap_smoke.rs::run_warm_without_owner_fails_without_cold_booting`
- `crates/m80-cli/src/cmds/warm/run.rs::tests::run_request_empty_pool_returns_pool_empty_without_cold_boot`
- `crates/m80-cli/tests/e2e_warm.rs::foreground_warm_owner_serves_run_and_drains_without_cold_fallback` (ignored real-KVM integration)
- `crates/m80-firecracker/tests/warm_pool.rs::reset_evidence_requires_every_input`
- `crates/m80-firecracker/tests/warm_pool.rs::reset_evidence_does_not_infer_from_partial_truth`
- `crates/m80-firecracker/tests/warm_pool.rs::warm_pool_allocates_pre_restored_slot_and_refills_after_discard` (ignored real-KVM integration)
