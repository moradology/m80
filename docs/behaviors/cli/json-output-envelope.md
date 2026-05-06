# CLI JSON Output Envelope

## Present-Tense Behavior

Every `m80 --json` document uses a format-versioned envelope:

```json
{
  "version": 1,
  "request_id": "req_01H...",
  "data": {}
}
```

`version` is an integer schema version for the envelope plus the payload shape
emitted by that command. Breaking changes bump the version. Callers must inspect
the version before treating `data` as a stable schema.
`request_id` is present when the command is running inside a scoped
`m80 run` invocation. Other subcommands omit it.

The contract applies to stdout payloads (`version`, `preflight`, `config show`,
`list`, `inspect`, `logs`, `env`, `cleanup`, and `run --json`) and to
structured stderr errors rendered when `--json` is set.

## Boundaries

Human-readable output is not enveloped. Normal `m80 run` pipe mode is also not
enveloped: guest stdout remains host stdout, guest stderr remains host stderr,
and the process exit code remains the guest exit code.
Plain wrapper errors include the same `request_id` on stderr when one exists.

There are no compatibility aliases for pre-envelope JSON because this contract
lands before external consumers exist.

## Verification

- `crates/m80-cli/src/json.rs` pins the helper-level envelope shape.
- `crates/m80-cli/tests/output_error_contract.rs` pins run-scoped request ids
  on JSON stderr envelopes.
- `crates/m80-cli/src/cmds.rs` tests the config JSON renderer.
- `crates/m80-cli/src/cmds_walk.rs` tests list and inspect JSON renderers.
- `crates/m80-cli/src/cmds_walk/logs.rs` and
  `crates/m80-cli/src/cmds/env.rs` test diagnostics/DX JSON payloads.
- `crates/m80-cli/src/errors.rs` tests structured error JSON under the shared
  envelope.
- `crates/m80-cli/tests/version_smoke.rs` and
  `crates/m80-cli/tests/feature_gap_smoke.rs` pin subprocess-visible JSON.
