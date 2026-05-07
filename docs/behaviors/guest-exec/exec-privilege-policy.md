# Exec Privilege Policy Decision

Behavior bead: `m80-3xwa.5.7`.

## Decision

m80 does not add capability, `no_new_privs`, or seccomp fields to
`ExecRequest` until there is a dedicated safe Rust boundary for applying them.

The desired ordering is security-sensitive:

1. fork;
2. optional namespace setup;
3. `setgroups`;
4. gid/uid transition;
5. capability bounding/permitted/effective/inheritable/ambient changes;
6. `PR_SET_NO_NEW_PRIVS` when required;
7. seccomp filter load at the correct point for the requested privilege model;
8. exec.

Encoding fields before m80 can apply that sequence safely would create a wire
promise with no reliable enforcement. Silent best-effort application is not
acceptable for an isolation boundary.

## Current Boundary

In m80 v0.x, VM-level isolation is the enforced boundary. Per-workload
capability and seccomp policy inside the guest is deferred. The adapter layer
may still choose a guest image whose userspace already drops privilege before
running the requested program, but that is image policy, not an m80 wire
contract.

## Future Shape

Before `ExecRequest` grows privilege-policy fields, m80 needs a small safe
wrapper crate that owns:

- capability set representation and validation;
- `setgroups`, uid/gid, and capability syscall ordering;
- `PR_SET_NO_NEW_PRIVS` behavior;
- seccomp filter loading through a safe API;
- tests that read `/proc/self/status` from the spawned child to prove `Cap*`
  and `Seccomp` state.

Only after that crate exists should `m80-proto` add the fields and bump
`PROTOCOL_VERSION`.
