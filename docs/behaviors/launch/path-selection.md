# Launch Path Selection

Bead: `m80-9wm35.4`

`m80-firecracker` does not decide at launch time by probing the host again.
`m80-preflight::run()` records one selected launch path in
`Discovery::chosen_launch_path`, and phase 9 follows that recorded result.

## Systemd Path

When preflight selects `LaunchPath::Systemd`, phase 9:

1. Builds a transient `systemd-run` invocation for the official Firecracker
   jailer.
2. Writes `jailer-systemd-unit` in the run directory before calling
   `systemd-run`.
3. Starts the unit and treats a non-zero `systemd-run` exit as
   `FcError::SystemdUnitCreateFailed`; known unit-create failures remove the
   marker before returning.
4. Waits for the official jailer's `<firecracker-bin-basename>.pid` file.
5. Records normal `jailer-state.json` pid state with `jailer_pid = 0` and the
   Firecracker pid from the jailer pid file.

The marker exists to close the crash window between unit creation and pid-state
persistence. Startup recovery checks the marker before classifying a plan-only
run directory as an orphan. It reads `ActiveState` through `systemctl show`:
`active`, `activating`, `reloading`, and `deactivating` are live systemd state;
`inactive` and `failed` allow orphan cleanup; an unsupported state or probe
error preserves the run directory.

## Wrapper Path

When preflight selects `LaunchPath::Wrapper`, phase 9 uses
`MaterializedJail::launch`, which execs `m80-jailer-harden` before the official
jailer. This path does not write `jailer-systemd-unit`. It still converges on
the same official jailer pid file and `jailer-state.json` contract.

## Common Contract

Both paths return `JailedFirecracker` to the rest of the lifecycle. Cgroup
enrollment, API-socket waiting, REST preboot configuration, process cleanup, and
stopped-run-dir cleanup stay path-independent after phase 9 returns.
