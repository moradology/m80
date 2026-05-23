# CLI Bug Report Bundle

Behavior capture for release/install epoch `m80-o3uh9`.

## Contract

`m80 bug-report` emits one JSON support bundle to stdout. It does not launch a
VM and does not require KVM to succeed.

The public command is:

```sh
m80 bug-report > m80-bug-report.json
```

For a specific VM run directory:

```sh
m80 bug-report --vm-id <vm-id> > m80-bug-report.json
```

The bundle contains:

- selected release/tag from the active install pointer and selected profile
- the same installed-state view as `m80 install-status`
- verifier/proof-cache diagnostics from installed metadata
- host prerequisite summary from the env/preflight diagnostic collector
- bounded host diagnostics and guest console tails when `--vm-id` is supplied
- a redaction report listing which local-sensitive classes were replaced

`--request-id <id>` filters included log lines by opaque request id. Log capture
is bounded by `--log-tail-lines`; values above 1000 fail closed instead of
emitting unbounded logs.

## Redaction

Before output, the bundle recursively redacts:

- GitHub credential prefixes such as `ghp_` and `github_pat_`
- SSH/private-key blocks
- `$HOME`, `$RUNNER_TEMP`, `$GITHUB_WORKSPACE`, and the current workspace path

The command writes the redacted bundle, not the raw intermediate data.

## Verification

- `crates/m80-cli/src/cmds/bug_report.rs` tests path/secret redaction, request
  filtering, log-tail truncation, and the fail-closed tail limit.
- `crates/m80-cli/tests/parse_args.rs` pins the command parse surface.
- `crates/m80-cli/tests/help_smoke.rs` pins the visible help surface.
- `scripts/test-release-url-contract.py` keeps the README/runbook public
  troubleshooting snippet on `m80 bug-report > m80-bug-report.json`.
