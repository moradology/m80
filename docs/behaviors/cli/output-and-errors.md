# CLI Output And Error Contract

## Present-Tense Behavior

`m80 run` has two output modes.

In normal pipe mode, the guest process is treated as the foreground process:

- guest stdout is host stdout
- guest stderr is host stderr
- guest exit status is the `m80` process exit status

The CLI does not add VM ids, run-root paths, lifecycle status, or other m80
metadata to stdout in pipe mode.
Stdout and stderr are copied as streaming chunks arrive; pipe mode is not
capped by the buffered exec response limit.

With global `--json`, successful structured command output is written to stdout
inside the versioned JSON envelope from
`docs/behaviors/cli/json-output-envelope.md`.

## Wrapper Failures

Failures in the wrapper itself render on stderr and return stable m80 exit
codes. Wrapper failures must not write stdout metadata.

The documented exit-code classes are:

- `1` generic runtime failure
- `2` preflight failure
- `3` admission refused
- `4` manifest failure
- `5` invalid lifecycle state
- `6` config failure
- `7` explicit v0.x feature gap
- `8` warm pool empty

With global `--json`, wrapper errors render one versioned JSON envelope on
stderr. The envelope's `data` object contains `variant`, `detail`, and
`exit_code`. For `m80 run`, both the outer JSON envelope and the error payload
carry the same opaque `request_id`.

Without `--json`, wrapper errors render as `error: [<request_id>] <detail>`
for `m80 run` so users can grep the run directory diagnostics and guest
console with the same token. Non-run commands do not mint a request id.

## Child Failure

A child process that exits nonzero is not a wrapper failure. It is still a
successful sandboxed exec from m80's perspective: the child's stdout/stderr are
passed through and the m80 process exits with the child exit code. KVM-backed
e2e coverage for the runtime passthrough path belongs to `m80-lt15.5`.

## Verification

- `crates/m80-cli/tests/output_error_contract.rs` pins pre-backend wrapper
  failure behavior, JSON stderr envelopes, feature-gap exit code behavior, and
  the documented `FcError` exit-code table from an integration-test boundary.
- `crates/m80-cli/tests/version_smoke.rs` pins successful `--json` stdout.
- `crates/m80-cli/src/cmds/tests.rs` and `crates/m80-cli/src/cmds_walk.rs`
  pin KVM-free successful JSON renderers for preflight, config, cleanup, list,
  and inspect.
