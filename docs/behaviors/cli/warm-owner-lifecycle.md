# CLI Warm Owner Lifecycle

Behavior capture for bead `m80-lt15.20`.

## Current Status

Foreground owner mode is implemented. `m80 warm enable --foreground` starts a
visible resident owner in the current process, fills a snapshot-backed
`WarmPool`, writes owner identity under the warm run-root, and serves a Unix
control socket. There is no hidden daemon behind `m80 run --warm`, and no
command silently cold-boots when warm capacity was requested. System service
mode remains reserved for a packaging bead.

## Supported Owner Modes

The implementation should support these modes, in this order:

1. Foreground developer owner (implemented):
   `m80 warm enable --foreground --size <n> [--profile <name>] [--egress none|outbound]`
2. System service owner (reserved):
   `m80 warm enable --system --size <n> [--profile <name>] [--egress none|outbound]`

User-service mode is explicitly deferred. Supporting both system and user
services in v0.2 would double the privilege, log, and run-root ownership matrix
before the single-host contract is proven.

The foreground owner is the first implementation target because it makes e2e
tests honest: a test starts a visible process, observes status, leases warm
capacity, drains it, and waits for that process to exit.

## Commands

Command surface:

```text
m80 warm enable --foreground --size <n> [--profile <name>] [--egress none|outbound]
m80 warm enable --system --size <n> [--profile <name>]
m80 warm status [--profile <name>]
m80 warm drain
m80 warm disable
m80 run --warm -- <program> [args...]
```

`enable --foreground` starts the explicit owner and fills to the target
ready-slot count before accepting leases. `enable --system` exits as a feature
gap until service packaging lands.
`status` reads owner state only; it does not start or recover an owner.
`drain` prevents new leases, waits for leased slots to finish or be discarded,
then deletes ready/filling slots. `disable` tears down the owner and removes
all owner-owned slots and control sockets.

Bare `m80 warm` should remain help/error surface, not an alias for `status`.

## Status JSON

`m80 warm status --json` returns a versioned JSON envelope with this data shape:

```json
{
  "owner": {
    "state": "available | unavailable | draining | disabled",
    "mode": "foreground | system | null",
    "identity": "pid:4242 | systemd:m80-warm.service | null"
  },
  "profile": {
    "requested": "minimal",
    "active": "minimal | null",
    "compatible": true
  },
  "slots": {
    "target_ready": 2,
    "ready": 1,
    "filling": 0,
    "leased": 1,
    "discarded": 3
  },
  "lifecycle": {
    "accepting_leases": true,
    "draining": false
  },
  "last_error": null,
  "paths": {
    "run_root": "/run/m80/warm",
    "log": "/run/m80/warm/owner.log"
  }
}
```

Unavailable owner status is not `PoolEmpty`. `PoolEmpty` means the owner is
compatible and running but has no ready slot. Owner unavailable, profile
mismatch, and disabled/draining states are distinct so scripts can decide
whether to start, wait, drain, or fall back by policy outside m80.

## Ownership And Paths

Foreground mode:

- owner process is the foreground `m80 warm enable --foreground ...` process
- control socket lives under the configured warm run-root
- logs go to stderr; owner identity lives in `owner.json` under the warm run-root
- shutdown is Ctrl-C/SIGTERM to the foreground owner or `m80 warm drain`

System mode:

- owner process is managed by a shipped `m80-warm.service` unit
- unit must run with the same privilege/capability model as normal `m80 run`
- logs go to journald plus the configured owner log path
- `disable` stops the unit and removes owner-owned state

Both modes must write an owner identity file containing binary version, profile
identity, artifact identity, warm run-root, control socket path, and start time.
`status` reads this file and refuses to infer compatibility from a socket alone.

## Profile Identity

Warm compatibility must include every input that makes a restored slot safe:

- selected profile name
- kernel image sha
- rootfs/base image sha
- image manifest schema, image kind, and kernel kind
- Firecracker version pin
- egress policy
- vCPU and memory sizing
- no-workspace status
- snapshot pair identity
- guestd ready-probe request

If the requested profile identity differs from the active owner identity,
`status` reports `profile.compatible=false`, `last_error.kind=profile_mismatch`,
and `m80 run --warm` fails before trying to lease. It must not cold-boot and
must not lease a slot from the wrong profile.

## Crash And Upgrade Behavior

If the owner crashes, callers see `owner.state=unavailable` until a new owner
starts and performs explicit recovery. Recovery must validate owner identity,
slot ownership, profile identity, and run-root contents before reusing anything.
If validation is incomplete, slots are discarded and refilled.

Binary, image, manifest, snapshot, sizing, egress, or profile changes
invalidate ready slots. The owner drains/deletes mismatched slots and refills
from the new identity before accepting leases.

## Idle, Drain, And Disable

Warm slots are owned by the resident owner, not by individual `m80 run`
processes. Slot idle shutdown is disabled or accounted for by the owner; a Ready
slot must not silently expire behind `status` counts.

Drain transitions:

1. set `accepting_leases=false`
2. stop filling new slots
3. delete ready slots
4. wait for leased slots to return or be discarded
5. remove owner-owned residue
6. report `draining=false` only after cleanup completes

Disable has the same cleanup requirement but also stops the owner process or
service. It is not a softer drain alias.

## Non-Goals

- No `m80 pool` command.
- No hidden daemon started by `m80 run --warm`.
- No cold fallback when owner is absent, empty, draining, disabled, crashed, or
  incompatible.
- No workspace-backed warm slots until attach-late workspace design lands.
- No PTY warm attachment until exclusive terminal lease ownership is designed.
- No inference from socket existence, PID liveness, or clean-looking run-root
  directories without complete owner/profile identity evidence.

## Verification

Current fixture tests live under `crates/m80-cli/src/cmds/warm/`:

- `status::tests::unavailable_owner_status_has_json_fields`
- `status::tests::incompatible_profile_status_fails_closed`
- `status::tests::drain_and_disable_transitions_are_distinct`
- `run::tests::run_request_empty_pool_returns_pool_empty_without_cold_boot`

Command-level coverage:

- `crates/m80-cli/tests/feature_gap_smoke.rs::warm_status_without_owner_is_unavailable_without_preflight`
- `crates/m80-cli/tests/feature_gap_smoke.rs::run_warm_without_owner_fails_without_cold_booting`
- `crates/m80-cli/tests/e2e_warm.rs::foreground_warm_owner_serves_run_and_drains_without_cold_fallback` (ignored real-KVM integration, including warm streaming)

System service/unit fixture validation remains future work because system mode
is still explicitly reserved.
