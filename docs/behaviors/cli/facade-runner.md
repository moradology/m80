# CLI Facade Runner

Behavior capture for bead `m80-lt15.2`.

## Contract

`crates/m80-cli/src/main.rs` is only the binary edge:

1. Parse argv with clap.
2. Call `m80_cli::runner::run`.
3. Exit with the returned code.

The dispatch table lives in `crates/m80-cli/src/runner.rs` so tests can exercise
CLI routing without spawning a subprocess for every case.

## Public Library Surface

`m80-cli` remains a binary product, not an embedding API. The lib target exposes
only the surfaces needed by the binary and tests:

- `args`
- `errors`
- `runner`

Implementation modules stay private:

- `cmds`
- `cmds_walk`
- `config`

## Error And Config Testability

Subcommands that hit host state convert their failures into `FcError` and render
through `crates/m80-cli/src/errors.rs` instead of leaking arbitrary `anyhow`
errors out to `main.rs`.

KVM-free tests cover the risky seams:

- runner dispatch without spawning `m80`
- preflight error-to-exit-code mapping
- config table rendering from an in-memory `EffectiveConfig`
- feature-gap exit codes before backend/preflight work

## Tests

- `crates/m80-cli/tests/facade_runner.rs`
- unit tests in `crates/m80-cli/src/runner.rs`
- unit tests in `crates/m80-cli/src/cmds.rs`
- `crates/m80-cli/tests/feature_gap_smoke.rs`
