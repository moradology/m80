# CLI Product Surface

## Present-Tense Behavior

m80 presents Firecracker as a constrained-process wrapper. The first command a
user sees is:

```sh
m80 run -- echo hello
```

Pipe mode treats the guest process like the foreground process:

- guest stdout is host stdout
- guest stderr is host stderr
- guest exit status is the `m80` process exit status

The VM is an implementation detail. User-facing policy is expressed as process
visibility and effects:

- filesystem visibility: `--workspace`, `--cwd`, `--scratch-size`
- write effects: `--writeback never|on-success|always`
- world/network visibility: `--egress none|outbound`
- environment visibility: `--env`, `--secret-env`
- runtime selection: `--profile`
- terminal mode: `--tty`, `-i`
- lifetime acceleration: explicit `m80 warm` owner plus `m80 run --warm`
- first-run setup: `m80 quickstart --artifact-url <release-tarball>` installs
  the selected image/profile artifacts, writes the installed default
  profile/config, and runs `m80 run -- echo hello` with the CLI default
  outbound egress policy
- diagnostics: typed errors, JSON envelopes, run-root `list`/`inspect`, and
  guest console capture, all correlated by the run-scoped opaque `request_id`

Runtime availability is explicit. The requested program must already exist in
the selected image/profile or in the visible workspace. m80 does not execute
host binaries, pull OCI images, or install packages implicitly.

## Boundaries

This surface is not Docker-compatible lifecycle UX. It has no hidden daemon,
no background process manager, and no implicit cold fallback from requested warm
execution. Agent semantics remain outside m80 core.

## Verification

- Top-level `README.md` opens with `m80 run -- echo hello` and constrained
  process language.
- `examples/echo-hello`, `examples/workspace-roundtrip`, and
  `examples/network-egress` provide runnable command-first examples.
- `crates/m80-cli/tests/help_smoke.rs` pins top-level help to the
  constrained-process model.
- `crates/m80-cli/tests/quickstart_smoke.rs` pins the release-tarball
  quickstart install path without requiring KVM.
