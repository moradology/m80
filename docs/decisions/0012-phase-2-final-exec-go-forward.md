# 0012 - Phase 2 Final-Exec Go-Forward Plan

## Context

`m80-92eor` investigated whether m80 should get Firecracker's official jailer
to own the final-exec-site hardening that Phase 1 cannot own directly.

The inputs are:

- `docs/exploration/firecracker-jailer-final-exec-audit.md`
- `docs/exploration/firecracker-upstream-landscape.md`
- `docs/exploration/firecracker-consumer-final-exec-survey.md`
- `docs/design/firecracker-jailer-final-exec-patch-proposal.md`
- `docs/decisions/0011-phase-2-final-exec-fallback.md`

The audit result is narrower than the original concern but not empty. The
official jailer already closes inherited fds, clears inherited environment,
owns chroot/cgroups/device-node setup, applies supported resource limits, and
execs Firecracker with the configured uid/gid. Firecracker itself owns
steady-state VMM seccomp. The remaining gap is inherited final process state at
the boundary where the jailer stops being the jailer and becomes Firecracker:
capability assertion, supplementary groups, inherited NNP, signal mask, umask,
and parent-death signal caveats.

## Decision

Pursue upstream Path A: ask Firecracker maintainers for an opt-in official
jailer flag, `--final-exec-hardening`, with the semantics documented in
`docs/design/firecracker-jailer-final-exec-patch-proposal.md`.

Do not pivot now to an m80-owned launcher. Do not fork Firecracker. Do not carry
a downstream patch by default. Do not treat Phase 2 as a blocker for the next
m80 release or v0.1 ship bar.

If upstream later ships an acceptable final-exec hardening flag, m80 will
hard-cutover: require that Firecracker/jailer version in preflight and always
pass the flag. Until then, m80's ship state remains Phase 1:

- systemd-first launch on supported hosts;
- `m80-jailer-harden` fallback on hosts without supported systemd;
- official jailer owns privileged setup and final exec;
- Firecracker owns VMM seccomp;
- m80 claims inherited launch-path hardening, not upstream-owned final-exec
  capset/seccomp/PDEATHSIG.

## Upstream Plan

The subsequent implementation epic should be opened only when someone is ready
to engage upstream. It should contain these gates:

1. Post an RFC issue or draft PR body derived from
   `docs/design/firecracker-jailer-final-exec-patch-proposal.md`.
2. Wait for maintainer direction on API shape, empty environment semantics,
   parent-death-signal exclusion, and direct `execve` implementation shape.
3. If maintainers give a positive direction signal, implement the upstream PR
   against Firecracker with jailer security integration tests that inspect
   `/proc/<firecracker-pid>/status`.
4. If upstream merges and releases the feature, bump m80's Firecracker floor,
   pass `--final-exec-hardening`, update preflight/install docs, and attach
   real-KVM proof.
5. If upstream rejects or stalls, apply ADR 0011 and accept the documented
   residual gap.

This ADR does not open that implementation epic. It describes the path so the
investigation can close without losing the next step.

## Success Criteria

Path A succeeds only when all of these are true:

1. Firecracker maintainers accept the API/semantics or provide an equivalent
   final-exec contract.
2. The feature lands with upstream jailer security tests.
3. A Firecracker release includes the feature.
4. m80 updates preflight/launch code to require and pass the feature.
5. m80 real-KVM proof shows launch still works and the final VMM process state
   matches the new contract.

Anything earlier than an upstream release is progress, not a local cutover.

## Point Of No Return

Use ADR 0011's stall definition:

- no positive maintainer direction within 90 days;
- two consecutive Firecracker minor releases pass with no clear merge path;
- maintainers redirect to a broad sandboxing design;
- maintainers reject official-jailer ownership of the final state.

At that point, m80 stops spending release-critical effort on Phase 2 and accepts
the documented gap unless ADR 0011's revisit conditions are triggered.

## Consequences

Release notes and docs must stay exact. The current system does not provide an
upstream-owned final-exec capset/seccomp/PDEATHSIG contract. It does provide a
systemd/wrapper launch hardening envelope before the official jailer, official
jailer privileged setup and uid/gid exec, and Firecracker VMM seccomp after
startup.

This keeps m80 on official Firecracker artifacts and avoids replacing a mature
jailer with a large m80-owned syscall surface for a finite residual gap.
