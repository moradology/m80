# CLI Diagnostic Logs

Behavior capture for bead `m80-oajh`.

## Contract

`m80 logs <vm-id>` is an out-of-band diagnostics reader for one persisted run
directory. It does not contact a running VM, does not use a vsock log channel,
and does not replay the wrapped process stdout/stderr from `m80 run`.

Normal `m80 run` pipe mode remains transparent: guest stdout is host stdout,
guest stderr is host stderr, and m80 lifecycle diagnostics stay in files under
the run directory.

## Inputs

```text
m80 logs <vm-id> [--follow] [--request-id <id>] [--since <RFC3339|UNIX_MS>]
```

The command resolves the effective run-root through the normal config chain and
reads:

- `<run_dir>/console.log` for guest console and guestd stderr capture
- `<run_dir>/diagnostics.jsonl` for host VM-lifecycle diagnostics

Missing log files are treated as empty. A missing run directory is a config
error.

## Rendering

Human output is line-oriented and tagged with `[host]` or `[guest]`. Host
records use `timestamp_unix_ms` from diagnostics JSONL. Guest structured stderr
lines use their RFC3339 timestamp. Records are sorted by timestamp when a
timestamp is available.

`--request-id <id>` returns only host or guest records carrying that exact
opaque request id. `--since` accepts either UNIX milliseconds or
`YYYY-MM-DDTHH:MM:SSZ`.

`m80 --json logs <vm-id>` emits the standard CLI JSON envelope. The payload has
`data.version: 1`, the selected `vm_id`, `run_dir`, and structured `records`.

`--follow` polls the same files and emits new records as they are appended.

## Verification

- `crates/m80-cli/src/cmds_walk/logs.rs` tests host/guest interleaving,
  request-id filtering with zero false positives, and `--since` parsing.
- `crates/m80-cli/tests/parse_args.rs` pins the command-line shape.
- `crates/m80-cli/tests/help_smoke.rs` pins the visible help surface.
