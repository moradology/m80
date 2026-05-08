# Compromised Firecracker Composition

The Layer 2 composition test treats Firecracker as fully compromised and runs
the jailer attack batteries as the payload. The public security claim is: if
the VMM process is breached, the m80 jailer boundary still blocks host
filesystem reach, host process reach, privileged network operations, privilege
escalation, resource starvation, and cross-tenant access.

The composition test is intentionally ignored by default because it requires
root, the official Firecracker jailer, `m80-jailer-harden`, cgroup v2, and a
musl `m80-attack-runner` binary. Its value is the named failure surface: a
failure identifies the exact attack primitive that escaped.

Evidence:

- `crates/m80-jailer/tests/defense_composition.rs::compromised_fc_blocked_by_jailer_alone`
