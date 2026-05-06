# CLI Feature Gaps

Behavior capture for bead `m80-lt15.7`.

## Contract

The CLI product is the process-wrapper facade:

```text
m80 run [OPTIONS] -- <program> [args...]
```

Surfaces that imply a resident VM manager, hidden process registry, daemon,
socket directory, or out-of-process control channel must not partially work by
accident. They either disappear from the clap surface or return a stable feature
gap exit before backend work starts.

## Removed Surfaces

These VM-front-door commands are removed and are not compatibility aliases:

- `m80 launch`
- `m80 exec`
- `m80 stop`
- `m80 snapshot`

Callers that need direct lifecycle control should embed `m80-firecracker` and
hold the returned `RunningSandbox` handle in-process. Out-of-process lifecycle
control requires an explicit IPC design; it is not hidden behind CLI aliases.

## Reserved Surfaces

These parse as intentional future shapes and exit 7 until their owning behavior
lands:

- `m80 run --allow-host <host>`
- `m80 run --allow-cidr <cidr>`
- `m80 run --mount-config <host>:<guest>[:ro]`
- `m80 run --keep-on-failure`
- `m80 warm enable --system`

The reserved paths render a human error on stderr, and with global `--json`
render a versioned JSON envelope on stderr whose `data` object contains
`variant: "NotImplemented"` and `exit_code: 7`.

The implemented PTY contract for `--tty` / `-t` / `-i` is captured in
`docs/behaviors/cli/interactive-pty.md`. PTY-specific invalid combinations
such as `--json --tty`, `--stdin --tty`, and `-i` without `--tty` fail as
configuration errors before backend work starts.

The implemented foreground warm-owner contract is captured in
`docs/behaviors/cli/warm-lifetime.md` and
`docs/behaviors/cli/warm-owner-lifecycle.md`. Only system service packaging
remains feature-gapped in the warm command family.

## Pool Boundary

There is no `m80 pool` command in this surface. Warm execution is exposed
through `m80 warm ...` and `m80 run --warm`. The CLI must not cold-boot under a
command that claims to use warm capacity.

## Tests

- `crates/m80-cli/tests/parse_args.rs` rejects removed commands.
- `crates/m80-cli/tests/feature_gap_smoke.rs` verifies reserved paths exit 7,
  including JSON mode, and verifies PTY invalid combinations fail as config
  errors before backend work.
- `crates/m80-cli/tests/facade_runner.rs` verifies feature-gap dispatch without
  spawning the binary.
- `crates/m80-cli/tests/help_smoke.rs` verifies the supported help surface; the
  removed commands do not have help pages.
