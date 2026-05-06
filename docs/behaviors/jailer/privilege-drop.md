# Jailer Privilege Drop

## configurable-uid-gid

`JailerConfig` carries the UID and GID that Firecracker will run as inside the
jail. m80 defaults this pair in the orchestrator configuration, and
`m80-jailer` rejects zero for either field with `JailerError::UidGidInvalid`.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`JAILER_UID` and `JAILER_GID` lines 25-26.

Test: `crates/m80-jailer/tests/plan_compute.rs::uid_zero_rejected`.
Test: `crates/m80-jailer/tests/plan_compute.rs::gid_zero_rejected`.

## track-pids

`MaterializedJail::launch` returns `JailedFirecracker` carrying both
`jailer_pid` and `firecracker_pid`, and it persists both values to
`jailer-state.json`. Stop, force-kill, recovery, and cgroup assignment use
those tracked pids rather than deriving liveness from socket paths.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs`
`JailerRuntimeState` lines 112-120 and validation lines 218-249.

Test: `crates/m80-jailer/tests/recover.rs::live_state_with_own_pid_returns_live_jail`.

## startup-check

m80 verifies jailer launch privilege once during process preflight, before
constructing the backend that launches VMs. `m80-jailer` assumes that privilege
has already been established; it does not run a per-launch sudo probe.

The accepted privilege states are effective root or the required Linux
capability set in the process effective set. Missing privilege is reported by
`m80-preflight` as `PreflightError::PrivilegeUnavailable`.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs`
`verify_jailer_launch_privilege` lines 847-853. m80 hard-cuts to the
centralized `m80-preflight` contract.

Test: `crates/m80-preflight/tests/preflight/kvm_and_os_gates.rs::privilege_gate_rejects_missing_capabilities`.

## typed-error

Privilege failure is a typed preflight error, not a panic and not a late
best-effort launch failure. The error carries the missing capabilities and a
hint that tells the operator to run as root or set capabilities on the m80
binary.

Source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs`
lines 850-852.

Test: `crates/m80-preflight/tests/error_hints.rs::privilege_unavailable_has_hint`.
