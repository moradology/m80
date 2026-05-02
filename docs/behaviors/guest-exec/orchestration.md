# Guest Exec — Process Orchestration Behaviors

## spawn

The guest daemon spawns the child process directly from the caller-supplied `program` + `args`, applying the optional `cwd` and `env` overrides from the request envelope without consulting any tool catalog.

- `cwd` is passed to `Command::current_dir` when present.
- `env` replaces the environment entirely (`env_clear()` then sets the pairs) when present; absent means inherit the guest environment unchanged.
- `stdin` bytes are written to the child's stdin pipe before it is closed; absent means stdin is `/dev/null`.

Source: dossier `02-sandbox-api-and-guest-proto.md` § Resolution + `03-guest-daemon.md` § Process orchestration; predecessor `crates/sandbox/agent-sandbox-local/src/executor.rs`.

Test: `m80-guestd/tests/handle_connection.rs::exec_with_stdin_round_trips` — passes stdin bytes and asserts stdout matches.

## capture

The daemon captures the child's stdout and stderr byte streams via two parallel reader threads and returns them as inline `Vec<u8>` payloads on the response envelope, paired with the exit code and timing metadata. Each stream is capped at 1 MiB.

Source: dossier `02-sandbox-api-and-guest-proto.md` § ExecutionResponse shape; predecessor `crates/sandbox/agent-sandbox-local/src/executor.rs` capture loop.

Test: `m80-guestd/tests/handle_connection.rs::exec_with_stdin_round_trips` — verifies stdout bytes are returned in the response.

## timeout

When the caller-supplied `timeout_ms` elapses before the child exits, the daemon sends `SIGKILL` (`child.kill()`), marks the response status as `TimedOut`, and returns whatever stdout/stderr was buffered before the kill.

Source: dossier `02-sandbox-api-and-guest-proto.md` § ExecutionResponse shape; predecessor `crates/sandbox/agent-sandbox-local/src/executor.rs` timeout-driven kill path.

Test: `m80-guestd/tests/handle_connection.rs::exec_with_timeout_returns_timed_out` — `sleep 60` + 100 ms budget returns `TimedOut` within 2 s wall clock.
