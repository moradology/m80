# Warm Owner Socket Boundary

Behavior capture for bead `m80-8emae.24`.

## Contract

The foreground warm owner exposes `owner.sock` only as a same-user local control
socket. Binding the socket is not the authorization boundary by itself:

- the socket file is chmodded to `0600` immediately after `bind`
- each accepted connection is checked with Linux peer credentials
- a peer whose UID differs from the owner process UID is rejected before any
  warm control request is parsed

Warm control requests are bounded. The owner sets a read timeout on every
accepted stream and refuses a request body larger than the configured control
request cap. The cap applies before serde parsing so a local peer cannot force
unbounded allocation with malformed JSON.

The bound socket and owner identity file are owned by an RAII guard. Clean drain
and disable paths still report cleanup errors through `remove_owner_state()`;
early returns and unwinding best-effort unlink both files in `Drop`.

## Non-Contract

The warm owner is still an in-process local owner for a trusted user account. It
does not turn `m80 warm enable --foreground` into a multi-tenant service API, and
it does not try to recover from `SIGKILL`, kernel OOM kill, or a caller with
write access to the warm run-root. Those require an external service boundary
and host policy, not tolerant socket probing.

## Verification

- `crates/m80-cli/src/cmds/warm/owner.rs::tests::owner_socket_is_restricted_to_owner_uid`
- `crates/m80-cli/src/cmds/warm/owner.rs::tests::same_uid_peer_is_authorized`
- `crates/m80-cli/src/cmds/warm/owner.rs::tests::owner_state_guard_unlinks_socket_and_identity`
- `crates/m80-cli/src/cmds/warm/control.rs::tests::oversized_owner_request_is_protocol_failure`
