# 0005: Firecracker /dev/kvm Residual Surface After InstanceStart

## Status

Accepted as a documented deferred gap.

## Context

m80 relies on Firecracker plus the official jailer as the layer-1 isolation
boundary. The jailer creates the chroot device surface Firecracker needs,
including `/dev/kvm`. Firecracker must keep KVM file descriptors open while the
VM runs, and Firecracker's seccomp policy is installed by Firecracker itself.

The current upstream model is a single seccomp policy for the Firecracker
process lifetime. That policy has to allow startup-time KVM ioctls before
`InstanceStart`, including VM and vCPU construction and guest-memory
registration. Once `InstanceStart` completes, the runtime need is narrower, but
m80 cannot replace Firecracker's already-installed seccomp policy from outside
the process.

A compromised Firecracker process that reaches an open `/dev/kvm` file
descriptor therefore retains a broad KVM ioctl attack surface. KVM ioctls are
kernel-facing primitives, so this remains relevant to the guest-escape threat
model even though the attacker has already compromised the VMM process.

## Decision

m80 will not attempt an LD_PRELOAD, ptrace, or wrapper-based phase-two seccomp
retrofit for v0.x.

The supported position is:

- document the residual `/dev/kvm` surface as a known limitation;
- keep using Firecracker's supported seccomp and jailer paths rather than a
  brittle host-side shim;
- track an upstream Firecracker feature for a post-`InstanceStart` tightened
  seccomp phase;
- revisit this decision only when Firecracker exposes a supported hook or when
  m80 intentionally owns a custom Firecracker build.

## Rationale

A host-side shim would become a security boundary around the VMM's most
kernel-sensitive file descriptor. That is a larger trust claim than m80 can
honestly make without owning Firecracker internals, syscall evolution, and
kernel-version-specific ioctl behavior. It would also need privileged real-KVM
proof, not just unit coverage.

The honest v0.x line is to keep the boundary small: m80 configures and runs
Firecracker, but Firecracker owns its in-process seccomp lifecycle. The gap is
recorded so operators do not infer that m80 removes `/dev/kvm` runtime ioctl
risk after boot.

## Consequences

- A Firecracker process compromise is still treated as a serious layer-1
  boundary failure, not as a fully contained event.
- m80 can still adopt Firecracker's advanced static seccomp filter separately;
  that is a different control from phase-specific post-startup tightening.
- Real mitigation requires upstream support or a deliberate custom
  Firecracker ownership decision.

## Follow-up Trigger

Reopen this decision when Firecracker supports installing or switching to a
post-`InstanceStart` seccomp profile, or when m80 decides to carry a patched
Firecracker binary as part of its trusted computing base.
