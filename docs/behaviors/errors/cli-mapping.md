# CLI Error Mapping

## exit-codes

The `m80` CLI maps wrapper failures to stable non-zero exit codes so scripts
can branch on outcome class without scraping stderr:

- `1` generic runtime failure
- `2` preflight failure
- `3` admission refused
- `4` manifest failure
- `5` invalid lifecycle state
- `6` configuration failure
- `7` explicit v0.x feature gap
- `8` warm pool empty

predecessor source:
`crates/sandbox/agent-sandbox-firecracker/src/errors.rs:149-948`; typed variants
drive the CLI-facing failure surface.

Test:
`crates/m80-cli/tests/output_error_contract.rs::documented_fc_error_classes_have_cli_exit_codes_and_payloads`.

## json-envelope

When `--json` is set, wrapper errors render to stderr as the shared versioned
envelope:

```json
{
  "version": 1,
  "data": {
    "variant": "Config",
    "detail": "config: ...",
    "exit_code": 6
  }
}
```

`variant` is the stable top-level `FcError` variant name. `detail` is
human-readable and may change with more precise messages; callers branch on
`version`, `variant`, and `exit_code`.

predecessor source: dossier `07-modules-essential-vs-hygiene.md` section
`errors.rs`.

Test: `crates/m80-cli/tests/output_error_contract.rs::json_wrapper_failure_uses_stderr_envelope_and_empty_stdout`
and `crates/m80-cli/src/errors.rs::tests::rendered_error_json_is_versioned`.

## stderr-informational

Stderr is not the failure signal. In pipe mode, guest stderr is copied
byte-for-byte to host stderr, and a guest process may exit zero after writing
stderr. Wrapper failure is indicated by the m80 process exit code and, when
`--json` is enabled, by the typed error envelope written last on stderr.

predecessor source: dossier `07-modules-essential-vs-hygiene.md` section
`errors.rs`.

Test: `crates/m80-cli/src/cmds/tests.rs::run_passthrough_copies_streaming_chunks_without_crossing_streams`
and the ignored KVM e2e
`crates/m80-cli/tests/e2e_run_passthrough.rs::run_passthrough_preserves_stdout_stderr_and_exit`.
