# Guest Exec — Vsock Listener Behaviors

## bind

The guest daemon binds a vsock listener on the configured port (default 9001) at startup and emits a `GUESTD_READY` marker to stdout before accepting any connections.

Source: predecessor `services/guestd-rs/src/main.rs:280-282` (`bind_vsock_listener` + `emit_vsock_ready_marker`).

Test: `m80-guestd/tests/parse_args.rs::port_flag_parsed_correctly` — exercises `--version` (clean path); vsock bind itself requires a guest VM (`#[ignore]`).

## accept-loop

The accept loop processes vsock connections one at a time: read the request envelope, run the exec, write the response, flush, close, then accept the next connection.

Source: predecessor `services/guestd-rs/src/main.rs:318-329` (`serve_vsock_connections`).

Test: `m80-guestd/tests/handle_connection.rs::exec_true_returns_completed` — handler completes one full request/response cycle.

## flush-before-close

The connection handler flushes the response writer and calls `nix::unistd::sync()` before the connection drops, so the host sees committed bytes and the workspace filesystem is in a consistent state.

Source: predecessor `services/guestd-rs/src/main.rs:487-490` (flush before drop) and dossier `03-guest-daemon.md` ("flushes filesystems, closes").

Test: `m80-guestd/tests/handle_connection.rs::exec_true_returns_completed` — full cycle: write_frame + flush + sync + close. Filesystem sync is always called at end of `handle_connection`.
