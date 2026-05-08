# Truncated Frame

## Behavior

A malicious guest peer can announce a valid in-cap frame length, write fewer
body bytes than promised, and close the channel. The host must not block
indefinitely waiting for the missing bytes. It surfaces the under-read as:

```text
FcError::Protocol(WireProtocolError::DisconnectBeforeTerminal {
    context: "streaming exec",
})
```

The affected request fails, diagnostics record the disconnect-before-terminal
protocol error, and sandbox cleanup still succeeds.

## Evidence

- `crates/m80-guestd-malicious/src/main.rs` mode `truncated_frame` writes a
  16-byte declared frame length, writes only a shorter partial body, flushes,
  and closes the accepted connection.
- `crates/m80-firecracker/tests/malicious/truncated_frame.rs::truncated_frame_returns_disconnect_before_terminal_without_stuck_reader`
  launches the malicious guestd image, sends a normal exec request from a
  helper thread, asserts the typed disconnect error arrives within 10 seconds,
  checks diagnostics, and tears down the sandbox.

The real-KVM test is ignored by default because it needs
`M80_MALICIOUS_ARTIFACT_DIR` to point at image artifacts built with
`m80-guestd-malicious` installed as `/m80-guestd`.
