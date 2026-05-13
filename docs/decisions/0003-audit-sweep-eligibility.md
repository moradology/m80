# 0003 — Audit-sweep eligibility

## Context

`m80-l020n.9` removed `MS_BIND` from
`crates/m80-jailer/src/plan.rs::bind_remount_flags()` during an audit sweep.
The change was small and superficially mechanical, but Linux requires
`MS_BIND | MS_REMOUNT` when remounting a bind mount to identify the target.
The follow-up incident is captured in
[`docs/postmortems/2026-05-12-ms-bind-and-sudo-escape.md`](../postmortems/2026-05-12-ms-bind-and-sudo-escape.md).

Audit-sweep methodology remains useful for statically verifiable findings:
typos, dead imports, unused variables, missing `#[must_use]`, redundant
`clone()`, missing typed error variants in a `match`, and doc drift. It is
net-negative on kernel-behavior-determining code because the verification
model is wrong. Those changes are correct or incorrect according to Linux,
Firecracker, `sudo`, or cgroup behavior, not according to local doctrine.

## Decision

Audit sweeps must classify their target hunks before editing. Sweep work that
touches sweep-ineligible patterns must split those hunks out and ship them as
single-purpose diffs with smoke evidence per the "Kernel-touching diffs
require smoke evidence" rule.

**Sweep-eligible patterns include:**

- `crates/*/src/error.rs` typed errors and statically verifiable variants
- `crates/*/src/types.rs` struct and enum shape, derives, serde attrs
- `crates/*/README.md` and `docs/**`
- `Cargo.toml` and `Cargo.lock` dependency changes, with separate smoke when
  the dependency affects runtime behavior
- Test files under `crates/*/tests/` and `tests/`
- Local cleanup around `pub fn` signatures, `#[derive(...)]`, imports, and
  obviously redundant clones or moves

**Sweep-ineligible patterns include:**

- `nix::mount::MsFlags`, `nix::sched::CloneFlags`,
  `nix::unistd::*ns*`, `nix::sys::signal::SigSet`,
  `nix::sys::prctl::*`
- `caps::Capability`, `prctl(PR_CAPBSET_DROP)`, and writes to
  `/proc/<pid>/uid_map`, `gid_map`, or `setgroups`
- `seccompiler`, `libseccomp`, and BPF program construction
- cgroup v2 controller writes such as `cgroup.subtree_control`, `cpu.max`,
  `memory.max`, `pids.max`, and `io.weight`
- `mount(2)`, `umount2(2)`, `pivot_root(2)`, and `chroot(2)`
- `clone(2)`, `unshare(2)`, `setns(2)`, `fork(2)`, and `execve(2)` syscall
  surfaces themselves
- `sudo` invocation wrappers in `scripts/`
- `scripts/smoke.sh` and `scripts/bench-*.sh`
- Anything in `crates/m80-jailer/src/plan.rs`,
  `crates/m80-jailer-harden/src/`, or
  `crates/m80-firecracker/src/lifecycle/exec.rs`

## Consequences

Sweep agents must skip or report ineligible hunks instead of modifying them.
Some formerly broad sweeps now become smaller static cleanup diffs plus
separate human-driven kernel or script diffs. That is acceptable. The cost of
splitting is lower than silently landing a kernel-behavior regression under a
mechanical-cleanup title.

The classifier is intentionally pattern-level, not crate-level. Most of
`m80-firecracker` is still eligible for ordinary sweeps; only the
kernel-behavior and wrapper surfaces need the stronger process.

## Alternatives Considered

**Ban audit sweeps entirely:** rejected. The method is net-positive on static,
locally verifiable cleanup.

**Require smoke evidence on every diff:** rejected. That makes text-only and
type-only sweeps too expensive and trains contributors to ignore the rule.

**Trust the agent to know the difference:** rejected. Agents confidently
misclassify when the prompt rewards producing a diff, especially when the
mistake looks like a doctrine improvement.
