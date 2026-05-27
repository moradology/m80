# Firecracker Jailer Final-Exec Audit

Date: 2026-05-27

Scope: read upstream Firecracker jailer source at the m80-pinned release and
enumerate what is still missing at the final transition into the Firecracker
VMM. Source pin: Firecracker v1.15.1,
`f82c0bd0f0a74015642a0d452880f3ad10147b14`.

Primary upstream files:

- [`src/jailer/src/main.rs`](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/src/jailer/src/main.rs)
- [`src/jailer/src/env.rs`](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/src/jailer/src/env.rs)
- [`src/firecracker/src/seccomp.rs`](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/src/firecracker/src/seccomp.rs)
- [`src/vmm/src/seccomp.rs`](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/src/vmm/src/seccomp.rs)

## Upstream Sequence

The official jailer does meaningful early sanitization before parsing arguments:
it closes inherited fds from 3 upward with `close_range(...,
CLOSE_RANGE_UNSHARE)` and removes inherited environment variables in
`sanitize_process()` before `Env::new(...)` is constructed
([main.rs#L257-L327](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/src/jailer/src/main.rs#L257-L327)).

`Env::run()` then:

1. Copies the Firecracker binary into the chroot.
2. Optionally joins a network namespace.
3. Applies supported resource limits.
4. Sets up requested cgroups.
5. Opens `/dev/null` before chroot when daemonizing.
6. Calls `chroot(...)`.
7. Creates the jailed directory and device-node surface.
8. Optionally daemonizes or enters a new PID namespace.
9. Executes Firecracker with `Command::uid(...).gid(...).exec()`
   ([env.rs#L641-L770](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/src/jailer/src/env.rs#L641-L770),
   [env.rs#L531-L547](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/src/jailer/src/env.rs#L531-L547)).

Firecracker itself owns VMM seccomp: unless disabled, command-line parsing
selects default advanced filters or a custom filter
([src/firecracker/src/seccomp.rs#L25-L60](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/src/firecracker/src/seccomp.rs#L25-L60)).
When a filter is installed, Firecracker first sets `PR_SET_NO_NEW_PRIVS`, then
loads the BPF program with `SECCOMP_SET_MODE_FILTER`
([src/vmm/src/seccomp.rs#L93-L137](https://github.com/firecracker-microvm/firecracker/blob/f82c0bd0f0a74015642a0d452880f3ad10147b14/src/vmm/src/seccomp.rs#L93-L137)).

## Coverage Table

| Desired directive | Upstream Firecracker coverage | m80 current coverage | Residual |
|---|---|---|---|
| File descriptors | Covered before argument parsing: fds >= 3 are closed with `close_range(..., CLOSE_RANGE_UNSHARE)`. | Wrapper also closes inherited fds before official jailer; systemd path starts with a clean unit boundary. | No Phase 2 patch needed for inherited fds. |
| Environment | Covered before argument parsing: inherited env vars are removed. | Systemd path passes `Environment=` empty; wrapper also clears before execing the official jailer. | No Phase 2 patch needed for inherited env. |
| Resource limits | Partially covered: official jailer supports `fsize` and `no-file`; m80 also wires systemd limits for nofile, fsize, nproc, memlock, address-space, core, and stack. | Covered for m80's required launch limits. | Upstream has narrower knobs, but this is not the final-exec gap. |
| `chroot`, devices, cgroups, netns | Covered by the official jailer before final exec. | m80 delegates these privileged setup steps to the official jailer. | No Phase 2 patch needed here. |
| Seccomp | Covered by Firecracker VMM startup, not by jailer final exec. Default advanced filters are used unless disabled; custom filters are supported. | m80 passes custom filters when configured and otherwise relies on Firecracker's default seccomp. | Do not propose duplicate jailer seccomp. The only remaining gap is the pre-Firecracker-start window before Firecracker applies its own filter. |
| `PR_SET_NO_NEW_PRIVS` | Firecracker sets NNP immediately before installing seccomp. The official jailer does not set an inherited NNP policy just before `exec()`. | Systemd launch sets `NoNewPrivileges=yes`; wrapper launch sets NNP before official jailer. | m80 has inherited coverage, but upstream does not expose a final-exec contract. |
| Capability state | Official jailer uses `Command::uid(...).gid(...)`; it does not explicitly clear ambient/inheritable caps or assert final effective/permitted state immediately before `exec()`. | Wrapper clears ambient/inheritable caps, prunes the bounding set, and retains only jailer setup capabilities before official jailer. Systemd constrains `CapabilityBoundingSet` and clears ambient capabilities. Live m80 proof has observed zero VMM caps after launch. | Finite residual: m80 still relies on inherited setup plus uid/gid transition, not an upstream-owned final-exec cap drop/assertion. |
| Supplementary groups | Official jailer sets uid/gid via `Command`; no explicit `setgroups([])` is present in the final exec path. | Wrapper calls `setgroups([])`; systemd passes `SupplementaryGroups=` empty. | m80 inherited coverage exists; upstream final-exec contract is absent. |
| Signal mask | No jailer reset was found. | Wrapper resets the signal mask before official jailer. Systemd does not create a Firecracker-owned final-exec signal-mask contract. | Finite residual for upstream. |
| Umask | No upstream jailer umask set was found in the final exec path. | Wrapper sets `0077`; systemd passes `UMask=0077`. | m80 inherited coverage exists; upstream final-exec contract is absent. |
| Parent-death signal | No `PR_SET_PDEATHSIG` use was found in the official jailer. | Wrapper sets `SIGKILL`; systemd uses unit lifecycle instead of a process parent-death signal. | Finite residual for upstream; systemd path should not claim PDEATHSIG semantics. |

## Residual Gap

The gap is not "the official jailer does no hardening." It already owns the
privileged setup sequence, inherited fd closure, env cleanup, chroot, cgroups,
namespace join, resource-limit subset, daemonization, and final uid/gid exec.
Firecracker itself owns steady-state VMM seccomp.

The remaining Phase 2 gap is narrower:

1. No upstream final-exec hook explicitly clears or asserts capability state
   after privileged jailer setup and immediately before Firecracker starts.
2. No upstream final-exec hook clears supplementary groups.
3. No upstream final-exec hook sets inherited `PR_SET_NO_NEW_PRIVS`; Firecracker
   sets NNP later when installing seccomp, and m80 sets NNP before invoking the
   official jailer.
4. No upstream final-exec hook resets the signal mask.
5. No upstream final-exec hook sets umask.
6. No upstream final-exec hook sets `PR_SET_PDEATHSIG`.

## Recommendation

Do not short-circuit the epic as "no patch needed." Also do not propose a broad
jailer seccomp redesign: Firecracker already owns VMM seccomp and m80 should keep
passing custom filters through the existing interface.

The plausible upstream request is a narrow final-exec hardening hook in the
official jailer, scoped to inherited process state that survives until
`Command::exec()`: supplementary groups, capability state, NNP, signal mask,
umask, and optionally parent-death signal. If upstream rejects that, m80 can
accept the documented residual rather than maintain a Firecracker fork, because
the current systemd/wrapper paths already cover the same directives before the
official jailer and live m80 proof has observed the resulting VMM state.
