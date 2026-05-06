# Delete And Run-Root Recovery

## Teardown Order

Normal lifecycle teardown is split across `RunningSandbox::stop` and
`StoppedSandbox::delete`.

`RunningSandbox::stop` consumes the running handle, sends the normal stop plan,
drops the jail and cgroup guards, unmounts any snapshot bind, carries the
scratch image into `StoppedSandbox`, and leaves the run directory intact for
optional change extraction or triage. `StoppedSandbox::delete` is the explicit
final step: it records the delete phase and removes the entire per-VM run
directory with `remove_dir_all`. If the run directory is already missing,
delete treats that as clean so release remains idempotent when explicit stale
recovery won the race.

Socket files, jailer state files, bind-mounted jail contents, and storage
artifacts are therefore removed as part of the whole run directory. Network TAP
cleanup is only relevant for outbound NAT; v0.1 rejects outbound NAT before VM
launch, so clean delete has no TAP to tear down in the no-egress path.

This is the m80 form of the older predecessor cleanup behavior recorded at
`crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:1308-1381`, but m80
uses Rust type-state boundaries rather than one large cleanup function.

## Recovery On Startup

`Backend::recover_stale_run_root()` is an explicit one-shot recovery pass over
the backend run-root. It is not hidden inside every `Sandbox::launch` call and
there is no background loop in `m80-firecracker` v0.1. CLI/admin surfaces call
it when cleanup is requested.

For each run-root child directory:

1. A live `ownership.lock` whose pid exists under `/proc` causes recovery to
   skip that directory.
2. A malformed `ownership.lock` is ambiguous and causes recovery to preserve
   that directory.
3. `.preserved/` is skipped because it contains explicit triage archives.
4. A persisted live jail state with no live m80 owner is treated as an orphaned
   VM; recovery SIGKILLs the jailer/firecracker pids and removes the run dir.
5. An orphan/no-jail directory is removed.
6. Ambiguous or unreadable jailer recovery state is logged and preserved.

Removal first detaches any mountpoints under the run directory using
`/proc/self/mountinfo`, then removes the orphan cgroup leaf, then removes the
directory tree. This keeps stale bind mounts and cgroups from leaking after a
crash.
