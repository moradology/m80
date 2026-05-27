# 0011 - Phase 2 Final-Exec Fallback

## Context

`m80-92eor` is the Phase 2 investigation for moving final-exec-site hardening
into Firecracker's official jailer. Phase 1 is already useful: systemd launch
and the wrapper fallback apply inherited hardening before the official jailer,
and Firecracker itself owns VMM seccomp after startup. The residual is narrower
than expected, but not empty:

- no upstream final-exec capability clear/assertion after privileged jailer
  setup;
- no upstream final-exec supplementary-group clear;
- no upstream final-exec inherited `PR_SET_NO_NEW_PRIVS` contract;
- no upstream final-exec signal-mask reset;
- no upstream final-exec umask contract;
- no upstream parent-death-signal contract, with caveats for daemonize and
  new-PID-namespace modes.

The upstream landscape is favorable for small, tested jailer changes and poor
for broad sandboxing features. Recent narrow jailer PRs merged quickly; broader
Landlock-style work is open/parked. The consumer survey found real public
interest in VMM process isolation, strongest in firecracker-containerd, but no
public evidence that another consumer is waiting on this exact final-exec flag.

## Decision

If upstream rejects or stalls the proposed official-jailer final-exec hardening
flag, m80 will **accept the documented residual gap** rather than fork
Firecracker, carry a downstream jailer patch, or build an m80-owned launcher.

That means Phase 1 remains the m80 ship state:

- systemd-first launch on supported hosts;
- `m80-jailer-harden` as the explicit fallback where systemd is unavailable;
- Firecracker official jailer remains the owner of chroot, cgroups, mknod,
  namespace entry, resource limits, and final exec;
- m80 claims inherited launch-path hardening and live observed VMM state, not
  upstream-owned final-exec hardening.

When an upstream Firecracker release does include an acceptable final-exec
hardening flag, m80 will hard-cutover: preflight requires that jailer version
and m80 always passes the flag. No long-lived compatibility branch.

## Stall Definition

Treat upstream as stalled if any one of these is true:

1. No maintainer gives a positive direction signal within 90 days of the RFC or
   draft PR.
2. The proposal misses two consecutive Firecracker minor releases with no clear
   maintainer-owned path to merge.
3. Maintainers redirect the feature to a much broader sandboxing design.
4. Maintainers reject the premise that the official jailer should own this
   final process state.

Stall is not failure. It just means m80 stops holding local release work hostage
for this improvement.

## Alternatives

| Alternative | Benefit | Cost | Decision |
|---|---|---|---|
| Fork Firecracker jailer | Full control over final exec site | Continuous merge work, CVE tracking, release rebuilds, and divergence from the official jailer trust path | Rejected |
| Downstream patch on upstream tags | Less work than a fork; one patch can be reapplied per release | Still blocks every Firecracker bump on patch maintenance and revalidation; creates a private security delta | Rejected unless a future concrete exploit/risk requires it |
| Accept documented gap | Zero new maintenance; keeps m80 on official Firecracker artifacts | Residual final-exec ownership remains outside m80 | Chosen fallback |
| Build m80-owned launcher | Complete local control | Large new syscall surface, safe-wrapper work, seccomp infrastructure, and inherited jailer CVE classes | Rejected for this residual gap |

## Revisit Conditions

Reopen this decision if any of these become true:

1. A real exploit, credible vulnerability report, or KVM smoke artifact shows
   Phase 1 inherited hardening is insufficient for m80's threat model.
2. Firecracker changes the official jailer so the current uid/gid drop or VMM
   seccomp assumptions no longer hold.
3. A major Firecracker consumer publicly asks for the same final-exec contract
   and upstream asks for a concrete implementer.
4. Upstream accepts the design but release timing alone blocks an urgent m80
   security release; in that case a short-lived downstream patch may be
   justified with an explicit removal target.
5. m80's target consumer raises its isolation bar from "strong inherited
   launch-path hardening plus official jailer" to "m80 must own the exact final
   exec syscall sequence."

## Consequences

This decision keeps release pressure bounded. The upstream proposal is worth
making because it would put the final boundary where it belongs, but m80 does
not need to hold v0.1 or the next installer release on it.

Documentation and release notes must keep the claim exact: systemd/wrapper
hardens the launch path and the official jailer/VMM do the final setup; m80 does
not currently own an explicit final-exec capset/seccomp/PDEATHSIG contract.
