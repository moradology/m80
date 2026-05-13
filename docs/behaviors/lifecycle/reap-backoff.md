# SIGKILL Reap Backoff

Bead: `m80-jp6ik.16`.

After sending `SIGKILL`, m80 polls `waitpid(WNOHANG)` for child Firecracker and
jailer processes until the process exits, another parent owns the reap, or the
existing two-second timeout expires.

The poll delay starts at 1 ms and backs off through 2 ms, 4 ms, 8 ms, and
16 ms before capping at 20 ms. That keeps the pathological timeout bounded
without imposing a fixed 20 ms minimum wait on the common case where SIGKILL
reaps within a few milliseconds.

Pinned tests:

- `reap_poll_backoff_starts_fast_and_caps_at_twenty_ms`
- `reap_sleep_never_exceeds_remaining_deadline`
- `reap_poll_schedule_observes_exit_within_next_backoff_slot`
