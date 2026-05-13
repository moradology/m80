# 2026-05-12 — MS_BIND, sudo escape, and stale bench images

**Status:** fixed; process follow-ups tracked under `m80-lovpn`.
**Time to root cause:** ~30 hours from false-green closure to the first
real-KVM bench run exposing the stack.

## Summary

Three real regressions landed in the perf-bench push: an audit sweep removed
`MS_BIND` from the bind-remount flags, `scripts/bench-cold-launch.sh` placed
the `m80_invoke` shell function behind `sudo`, and
`IMAGE_BUILD_DIR_MINIMAL` still defaulted to a stale mock image directory.
All three bypassed `cargo test`, shellcheck-class lint coverage, and the
mock-based "e2e" layer. They were caught only when a human ran the bench
under `sudo` on a host with real KVM access.

## Timeline

- `m80-l020n.9` closed as an audit-sweep cleanup. During that sweep,
  `crates/m80-jailer/src/plan.rs::bind_remount_flags()` lost `MS_BIND`
  based on the claim that Linux ignores it on remount.
- The `m80-ekbk` perf-bench epic closed with 162 children green on mock data.
  The harness compute layer existed, but the committed measurements did not
  exercise the kernel, `sudo`, or the real image filesystem.
- The user pushed back on the mock-based proof. The mock binary and
  `TEST_MODE` branches were removed.
- The first real-KVM bench attempt failed with exit code 1: `sudo` launched a
  fresh shell that could not see the parent shell's `m80_invoke` function.
- The second attempt failed with exit code 2: a stale
  `IMAGE_BUILD_DIR_MINIMAL` default pointed at empty mock stub files whose
  mere existence passed the script's preflight.
- The third attempt failed with `EBUSY`: the bind remount needed
  `MS_BIND | MS_REMOUNT` to identify the bind mount target.
- Restoration commits landed for the sudo invocation, stale image default,
  and `MS_BIND` bitset.

## Root Cause

The tests did not touch the same kernel, `sudo`, and filesystem substrate
that production touched. They were certifying orchestration around
substitutes, not the behavior the bench actually needed to prove.

## Why Audit Sweep Made It Worse

An LLM audit agent applying a doctrine fix can create a diff that another LLM
agent will approve using the same doctrine. The `MS_BIND` change looked small,
mechanical, and plausibly supported by a local reading of docs. Its
correctness was actually defined by Linux mount-table behavior, not by the
Rust type system or a comment.

The missing gate was a real smoke against the kernel. `cargo test` could not
observe the mount behavior. Shellcheck could not observe it either. The
mock-based "e2e" path explicitly avoided the host primitives that would have
failed.

## What We're Changing

Diffs that touch mount flags, namespace flags, capability sets, signal masks,
seccomp filters, cgroup controller writes, `sudo` wrappers, or the smoke and
bench scripts must include a green real-KVM smoke paste in the PR. Audit
sweeps touching those categories are not eligible sweep work; they split into
single-purpose diffs with substrate evidence.

The immediate rules live in `AGENTS.md` and `CLAUDE.md`. Follow-up beads under
`m80-lovpn` add bitset regression tests, explicit verified-close discipline
for measurement beads, observable-shaped acceptance criteria, shellcheck
gating, and an audit-sweep eligibility classifier.

## What We Considered And Did Not Do Yet

**Privileged CI runner:** highest correctness payoff for kernel-touching diffs,
but it carries real operational cost. We will revisit it on recurrence or when
the runner can be maintained intentionally.

**Rewrite the bench harness in Rust:** attractive long-term if shell scripts
keep hiding defects, but premature for this incident. The current shell harness
now reaches real KVM and produced useful measurements once the smoke-blocking
bugs were removed.

## Lessons

**Mocks lie by construction.** They are useful for shape and plumbing, but a
mock that avoids the kernel, `sudo`, and real files cannot certify a bench
whose purpose is to measure those layers.

**Closing a bead is not evidence.** A measurement epic closes on committed
real numbers from the named substrate, not on a working harness skeleton.

**Audit methodology needs an eligibility boundary.** Static sweeps are useful
for static facts. Kernel-behavior-determining code needs observation.

## References

- Follow-up epic: `m80-lovpn`
- Regression source: `m80-l020n.9`
- Affected code: `crates/m80-jailer/src/plan.rs`,
  `scripts/bench-cold-launch.sh`, `scripts/bench-extras.sh`
