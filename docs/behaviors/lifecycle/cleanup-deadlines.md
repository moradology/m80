# Lifecycle Cleanup Deadlines

`RunningSandbox::stop` and `RunningSandbox::force_kill` do not wait forever for
host-side teardown helpers after the VM has been asked to stop or killed.

The lifecycle watcher join is bounded by `WATCHER_JOIN_TIMEOUT = 2s`. If the
watcher does not join in that budget, m80 logs an error and detaches the join
waiter so the caller can continue through teardown.

`MaterializedJail::drop` runs on a cleanup thread with a fixed deadline. If
jail unmount/rmdir cleanup exceeds the deadline, the caller continues and m80
logs `FcError::CleanupDeadlineExceeded { phase: JailDrop, .. }`. The detached
cleanup thread may still finish later.

`StoppedSandbox::delete` budgets both outbound network cleanup and per-VM
run-directory deletion with `RUN_DIR_DELETE_TIMEOUT = 5s`. A deadline miss
returns `FcError::CleanupDeadlineExceeded` with `phase` set to
`NetworkCleanup` or `RunDirDelete`; the cleanup worker is detached rather than
blocking the lifecycle caller indefinitely.

Host SIGKILL teardown retries once on `FcError::ReapTimeout`. If the process is
still live after the second SIGKILL and reap budget, the original
`ReapTimeout` class is returned to the caller.

These deadlines are fail-closed for the caller: m80 reports timeout instead of
silently treating an unfinished cleanup as complete, while still preventing an
unbounded stop/delete hang.
