# CLI Interactive PTY

Behavior capture for beads `m80-lt15.12` through `m80-lt15.15`.

## Current Status

`m80 run --tty` / `-t` is implemented as the terminal lane for `m80 run`.
`-i` is implemented as the live-input companion flag and is valid only with
`--tty` / `-t`. The CLI owns host terminal raw-mode setup/restoration, resize
forwarding, stdin/stdout bridging, and exit-code mapping; `m80-firecracker`,
`m80-proto`, and `m80-guestd` own the lower PTY request, frame, and guest
process behavior.

## User Shape

PTY mode keeps the process-wrapper mental model: one command runs inside a
sandbox with explicit filesystem, environment, and egress visibility. The VM is
not the product surface.

The interactive shape is:

```text
m80 run -it --workspace . --egress outbound --secret-env ANTHROPIC_API_KEY -- claude
```

`-t` / `--tty` allocates a guest terminal. `-i` connects the host terminal input
to that guest terminal after saving the host terminal mode and entering raw
mode. The selected guest image or visible workspace must already contain the
requested program; m80 does not install `claude`, pull an OCI image, or execute
a host binary.

## Pipe Mode Versus PTY Mode

Pipe mode is the default and remains the machine-friendly path:

- guest stdout is streamed to host stdout
- guest stderr is streamed to host stderr
- `m80 --json run ...` can print a buffered exec response
- `--stdin` preloads bytes, sends them to the guest child, then sends EOF

PTY mode is terminal-friendly instead:

- guest stdout and stderr are merged by the terminal device and are not
  separable
- terminal bytes stream directly between the guest PTY and the host terminal
- `--stdin` is not used for live keyboard input; `-i` owns that behavior
- `m80 --json run -t ...` is invalid because live terminal output is not a JSON
  document

Wrapper diagnostics still render on stderr before entering raw mode or after
restoring the terminal. While raw mode is active, the terminal byte stream is
owned by the guest process.

## Terminal Setup

The CLI saves the host terminal state before enabling raw mode and restores it
on normal exit, wrapper error, signal-triggered cancellation, and panic-unwind
paths where Rust cleanup can run. Wrapper diagnostics render to stderr; while
raw mode is active, terminal bytes from the guest are written to stdout.

PTY mode automatically projects only non-secret terminal metadata:

- initial terminal size
- subsequent terminal resize events
- `TERM`, `COLORTERM`, `LANG`, `LC_ALL`, and `LC_CTYPE` when present on the
  host and not already set by `--env` or `--secret-env`

No other host environment, dotfile, shell startup file, SSH agent, package
manager credential, editor state, or home-directory config is inherited. Tokens
and config files remain explicit through `--secret-env`, `--env`, and the
future `--mount-config` surface.

## Manual Claude Code Check

Claude Code is just a terminal process from m80's point of view. A manual check
therefore names every piece of host visibility explicitly:

```text
m80 run -it \
  --profile <profile-with-claude-and-runtime> \
  --workspace . \
  --cwd /workspace \
  --egress outbound \
  --secret-env ANTHROPIC_API_KEY \
  -- claude
```

Required inputs:

- workspace: `--workspace .` or another explicit host directory
- outbound egress: `--egress outbound`
- runtime/profile: the selected profile or rootfs must already contain
  `claude`, Node/runtime dependencies, shell utilities, and any certificates it
  needs
- auth: `--secret-env ANTHROPIC_API_KEY` or another explicit credential shape
- config/env: use `--env KEY=VAL`, `--secret-env KEY`, or a profile/rootfs that
  already contains non-secret config; config-file mounting remains reserved for
  the future `--mount-config` behavior

m80 does not install `claude`, pull a package manager cache, mount `$HOME`, read
dotfiles, forward SSH agents, or infer credentials from the host login session.
Failure triage starts with the run-root diagnostics: `state.json`,
`diagnostics.jsonl`, and `console.log` carry the run directory, request ids, and
guest-side logs where available.

## Signal And EOF Semantics

Terminal-generated control input belongs to the guest foreground process.
Common examples:

- Ctrl-C is written through the terminal path so the guest foreground process
  receives the normal terminal interrupt
- Ctrl-D sends EOF on the terminal input path
- Ctrl-Z and job-control bytes are guest terminal behavior, not host wrapper
  subcommands

Host wrapper termination is separate. If the wrapper receives a non-terminal
shutdown signal, loses the host terminal, or drops the PTY channel, the
implementation must cancel the guest foreground process group/session and then
clean up the sandbox. This depends on the run cancellation and process-tree
beads; PTY implementation must not leave a guest child alive after the wrapper
has gone away.

## Required Protocol And API Work

`m80-proto` exposes a terminal exec mode that is distinct from pipe exec. The
wire contract carries:

- one request id for the PTY session
- terminal input bytes from host to guest
- terminal output bytes from guest to host
- terminal resize events
- cancellation/disconnect semantics
- exactly one terminal exit result with the guest process status

The protocol must not pretend PTY output has separated stdout/stderr streams.
Backpressure must be explicit enough that a slow host terminal does not require
unbounded buffering in guestd or the host.

The implemented frame shape is documented in
`docs/behaviors/wire-protocol/pty.md`.

`m80-guestd` allocates the PTY, spawns the requested command as the foreground
process for that terminal, applies the initial size, bridges input and output,
handles resize events, and terminates the guest process group/session on
disconnect, timeout, or wrapper cancellation.

`m80-firecracker` exposes a synchronous PTY execution API that bridges the
terminal frames without requiring callers to understand Firecracker lifecycle
details. Buffered `exec()` and streaming pipe `exec_streaming()` remain separate
APIs because they preserve stdout/stderr separation.

`m80-cli` owns host terminal raw-mode setup, restoration, SIGWINCH forwarding,
stdin/stdout bridging, and final exit-code mapping.

## Invalid Combinations

These combinations must fail before backend work starts:

- `--json` with `--tty` or `-t`
- `--stdin` with `--tty` or `-t`
- `-i` without `--tty` / `-t`
- `--warm` with `--tty` / `-t` until the warm owner supports exclusive terminal
  attachment to a leased slot
- detached/background terminal mode; no such surface exists in v0.1

`--workspace`, `--cwd`, `--env`, `--secret-env`, `--egress`, and `--writeback`
keep their normal meanings in PTY mode. `--writeback` still requires
`--workspace` and runs after the guest process exits.

## Tests

Current evidence:

- `crates/m80-cli/tests/parse_args.rs::parse_run_pty_flag_shape`
- `crates/m80-cli/src/cmds/tests.rs::run_cwd_env_and_terminal_size_map_to_pty_request`
- `crates/m80-cli/src/cmds/pty.rs` unit tests for raw-mode restoration, raw
  terminal output copying, resize forwarding through a test seam, and signal
  exit-code mapping
- `crates/m80-cli/tests/feature_gap_smoke.rs` tests invalid `--json --tty`,
  `--stdin --tty`, and `-i` without `--tty` combinations before backend work
- `crates/m80-cli/tests/e2e_run_passthrough.rs::run_tty_smoke_uses_guest_terminal_and_preserves_exit_code`
  is an ignored real-KVM terminal smoke
- `crates/m80-cli/tests/e2e_tty.rs::interactive_tty_probe_reads_input_writes_ansi_and_preserves_exit_code`
  is an ignored real-KVM interactive PTY smoke using a host pseudo-terminal
- `crates/m80-proto/tests/pty_round_trip.rs`,
  `crates/m80-guestd/tests/pty_exec.rs`, and `cargo test -p m80-firecracker --lib`
  cover the lower protocol, guest runner, and host lifecycle API wiring
