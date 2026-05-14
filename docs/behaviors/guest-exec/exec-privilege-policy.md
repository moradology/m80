# Exec Privilege Policy

Behavior beads: `m80-3xwa.5.7`, `m80-8emae.25`.

## Decision

m80 does not add caller-controlled capability, UID/GID, or seccomp fields to
`ExecRequest` until there is a dedicated safe Rust boundary for applying them.
Instead, root-running `m80-guestd` applies one default workload profile to every
pipe exec and PTY exec:

- UID 1000 and GID 1000;
- supplemental groups exactly `[1000]`;
- `PR_SET_NO_NEW_PRIVS`;
- empty effective, permitted, inheritable, ambient, and bounding capability
  sets.

Developer/test launches where guestd is already non-root spawn directly because
there is no guest-root privilege to remove.

The implemented ordering is security-sensitive:

1. spawn a hidden `/proc/self/exe --m80-exec-shim <program> ...` child;
2. set `PR_SET_NO_NEW_PRIVS`;
3. clear ambient and bounding capabilities while still privileged;
4. set supplemental groups;
5. set real/effective/saved GID and UID to 1000;
6. clear effective, permitted, and inheritable capabilities;
7. exec the requested workload program.

The self-exec shim avoids `pre_exec` because the workspace forbids `unsafe`.
Silent best-effort application is not acceptable for this boundary: any failed
drop step fails the child before the requested workload runs.

## Current Boundary

In m80 v0.x, every root-guestd workload receives the fixed non-root profile
above. The wire does not expose a privileged mode and does not expose per-call
policy. Workloads that require package installation, mount, route mutation, or
other guest-root operations must be handled by the selected guest image or by a
future explicit m80 contract, not by an implicit fallback.

## Future Shape

Before `ExecRequest` grows caller-controlled privilege-policy fields, m80 needs
a small safe wrapper crate that owns:

- capability set representation and validation;
- `setgroups`, uid/gid, and capability syscall ordering;
- `PR_SET_NO_NEW_PRIVS` behavior;
- seccomp filter loading through a safe API;
- tests that read `/proc/self/status` from the spawned child to prove `Cap*`
  and `Seccomp` state.

Only after that crate exists should `m80-proto` add the fields and bump
`PROTOCOL_VERSION`.
