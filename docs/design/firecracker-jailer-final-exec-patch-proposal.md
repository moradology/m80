# Firecracker Jailer Final-Exec Patch Proposal

Date: 2026-05-27

This is the concrete upstream proposal produced by `m80-92eor.4`. It is not
posted upstream yet. It is intentionally narrower than "add more sandboxing":
Firecracker already owns VMM seccomp, and the official jailer already owns
chroot, cgroups, device nodes, fd cleanup, and environment cleanup. The missing
piece is a final-exec process-state contract immediately before the official
jailer becomes Firecracker.

Inputs:

- `docs/exploration/firecracker-jailer-final-exec-audit.md`
- `docs/exploration/firecracker-upstream-landscape.md`
- `docs/exploration/firecracker-consumer-final-exec-survey.md`

## Proposed Upstream API

Add one official jailer flag:

```text
--final-exec-hardening
```

Default when omitted: preserve current upstream behavior.

When present: the official jailer applies a fixed, documented hardening bundle
after all privileged jailer setup is complete and immediately before it execs
the Firecracker VMM.

This should be a single boolean flag, not a config file or a matrix of
independent toggles. The state being controlled is one conceptual boundary:
"before this process stops being the jailer and starts being Firecracker." A
single flag is easier for Firecracker maintainers to review and for downstream
orchestrators to test.

m80 behavior after a Firecracker release contains this feature: require that
jailer version in preflight and always pass `--final-exec-hardening`. No
compatibility path in m80.

## Semantics

With `--final-exec-hardening`, the official jailer applies these directives in
the final process after chroot/device/cgroup/resource setup and before
`execve(firecracker)`:

1. Clear supplementary groups with `setgroups([])`.
2. Clear ambient and inheritable capabilities.
3. Drop to the configured jailer `gid` and `uid`.
4. Assert the post-drop effective, permitted, inheritable, and ambient
   capability sets are empty; fail before exec if any are non-empty.
5. Set `PR_SET_NO_NEW_PRIVS=1`.
6. Reset the signal mask to empty.
7. Set `umask(0077)`.
8. Execute Firecracker with an empty environment.

The proposal deliberately does not include `PR_SET_PDEATHSIG` in the first
upstream request. Its semantics conflict with the official jailer's daemonize
and new-PID-namespace modes, where the process that would become the parent can
exit by design. If upstream wants to support it later, it should be a separate
flag that is rejected when `--daemonize` or `--new-pid-ns` is also present.

The proposal also deliberately does not include jailer-managed seccomp. With
default settings, Firecracker selects its advanced seccomp filters and sets NNP
immediately before installing them during VMM startup. m80 should not ask the
official jailer to duplicate or replace that VMM-owned filter.

## Implementation Shape

The current upstream `exec_command` uses Rust `Command` with `.uid(...)`,
`.gid(...)`, inherited stdio, extra args, and `.exec()`. The proposed feature
should make the ordering explicit rather than depend on undocumented setup
ordering inside `CommandExt`.

Concrete shape:

- Add `final_exec_hardening: bool` to `Env`.
- Parse `--final-exec-hardening` in `build_arg_parser()`.
- Replace the final `Command::uid(...).gid(...).exec()` path with a small
  helper that performs the credential and hardening sequence directly in the
  current process, then calls `execve`.
- Keep existing stdio behavior: stdin/stdout/stderr are inherited unless the
  existing daemonize path already redirected them to `/dev/null`.
- Preserve extra Firecracker args exactly as today.
- Return typed jailer errors for every failed syscall or failed cap assertion.

Pseudo-order:

```text
if final_exec_hardening {
    setgroups([])
    clear ambient capabilities
    clear inheritable capabilities
}

setgid(configured_gid)
setuid(configured_uid)

if final_exec_hardening {
    assert effective/permitted/inheritable/ambient caps are empty
    prctl(PR_SET_NO_NEW_PRIVS, 1)
    pthread_sigmask(SIG_SETMASK, empty)
    umask(0077)
}

execve(firecracker_path, argv, empty_env_if_hardened_else_current_env)
```

This keeps privileged setup untouched. It only changes the final process state
at the point where the official jailer has no more privileged work to do.

## Backward Compatibility

Upstream default stays current behavior. Existing jailer callers do not see the
new policy unless they opt in.

Known possible behavior changes for opt-in callers:

- Firecracker-created files use umask `0077`.
- `NoNewPrivs` becomes `1` before VMM startup.
- Supplementary groups are empty.
- Inherited environment is empty even if an outer launcher provided variables.
- Launch fails if final capabilities remain after the uid/gid drop.

These are the point of the flag. They should be documented as opt-in behavior.

## Multi-Consumer Fit

This is not m80-specific. It benefits any orchestrator that launches the
official jailer and wants Firecracker to own the final VMM process state instead
of relying on an outer wrapper.

Public supporting evidence:

- firecracker-containerd documents VMM process jailing as part of host security
  posture, even though its current implementation uses a runc-based jailer path.
- Kata Containers supports Firecracker and documents VMM/host-kernel isolation
  risk, capabilities, namespaces, and seccomp as part of the larger container
  isolation model.
- Lambda is broad motivation for Firecracker isolation, but should not be cited
  as needing this specific flag because its launch code is not public.

## Test Methodology

Unit-level checks:

- Argument parser accepts `--final-exec-hardening` and defaults to false.
- Hardening helper rejects simulated syscall failures with typed errors.
- Hardening helper builds the exact argv passed to Firecracker.

Integration test in `tests/integration_tests/security/test_jail.py`:

1. Launch a jailed Firecracker process with `--final-exec-hardening`.
2. Read `/proc/<firecracker-pid>/status`.
3. Assert:
   - `NoNewPrivs: 1`
   - `CapInh`, `CapPrm`, `CapEff`, and `CapAmb` are zero
   - `SigBlk` is zero
   - `Umask` is `0077`
   - supplementary groups are empty or only the configured primary gid,
     depending on kernel `/proc` rendering
4. Assert Firecracker still boots and can service the existing jailer security
   integration path.

Non-goal tests:

- Do not test jailer-managed seccomp; Firecracker's existing VMM seccomp tests
  own that.
- Do not include parent-death-signal tests in the first PR.

## Draft Upstream Issue Body

````markdown
## Summary

I would like to propose an opt-in official jailer flag,
`--final-exec-hardening`, that applies a small fixed process-state hardening
bundle immediately before the jailer execs the Firecracker VMM.

This is not a request for a new sandboxing subsystem and not a replacement for
Firecracker's VMM seccomp filters. The official jailer already owns chroot,
cgroups, device nodes, inherited fd cleanup, and environment cleanup. The gap is
the last boundary where the jailer has finished privileged setup and is about to
become Firecracker.

## Proposed API

Add:

```text
--final-exec-hardening
```

Default when omitted: preserve current behavior.

When present, after all privileged setup is complete and immediately before
`execve(firecracker)`, the jailer should:

1. Clear supplementary groups.
2. Clear ambient and inheritable capabilities.
3. Drop to the configured gid/uid.
4. Assert effective/permitted/inheritable/ambient capabilities are empty.
5. Set `PR_SET_NO_NEW_PRIVS=1`.
6. Reset the signal mask to empty.
7. Set `umask(0077)`.
8. Execute Firecracker with an empty environment.

I am intentionally not proposing jailer-managed seccomp here. Firecracker already
selects default/custom VMM seccomp filters and sets NNP before installing them
at VMM startup.

I am also intentionally not proposing `PR_SET_PDEATHSIG` in this first change
because its semantics interact poorly with `--daemonize` and `--new-pid-ns`.
That can be discussed separately if maintainers want it.

## Motivation

Outer orchestrators can apply inherited process hardening before invoking the
official jailer, but the official jailer still owns the final exec into
Firecracker. An official final-exec contract lets orchestrators rely on
Firecracker's own jailer for the final VMM process state rather than carrying a
wrapper or downstream patch.

This benefits any launcher that wants the official jailer to own the complete
transition into the VMM. Public adjacent evidence includes
firecracker-containerd documenting VMM process jailing as a host-security goal,
and Kata Containers documenting VMM/host-kernel isolation risks for Firecracker
and other hypervisors.

## Compatibility

The flag is opt-in. Existing jailer users keep current behavior unless they pass
`--final-exec-hardening`.

Opt-in callers should expect:

- `NoNewPrivs: 1`
- empty supplementary groups
- zero final effective/permitted/inheritable/ambient capabilities
- empty signal mask
- umask `0077`
- empty environment

## Test plan

Add a jailer security integration test that launches Firecracker with
`--final-exec-hardening`, reads `/proc/<firecracker-pid>/status`, and asserts
`NoNewPrivs`, capability sets, signal mask, umask, and supplementary groups.
The test should also prove the VM still starts through the existing jailer
security path.

Before sending a PR I would like maintainer feedback on:

1. Is a single opt-in boolean flag the right API shape?
2. Should empty environment be part of this final-exec contract, given the
   jailer already clears env early?
3. Should parent-death signal be excluded from this first patch, as proposed?
4. Would maintainers prefer a direct `execve` helper over relying on
   `CommandExt` setup ordering?
````
