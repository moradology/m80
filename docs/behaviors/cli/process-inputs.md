# CLI Process Inputs

Behavior capture for bead `m80-lt15.17.3`.

## Environment

`m80 run --env KEY=VAL` adds one explicit guest environment override. The flag
may be repeated, and an empty value is valid (`--env EMPTY=`).

`m80 run --secret-env KEY` copies one explicitly named host environment variable
into the guest environment without putting the value on the command line. The
host variable must exist; missing variables fail as wrapper configuration
errors before backend work starts.

The CLI does not automatically copy the host environment into the guest. When at
least one `--env` or `--secret-env` pair is supplied, guestd runs the child with
exactly the provided pairs after `env_clear()`. When no environment pair is
supplied, no CLI env projection is requested and guestd uses its baseline guest
environment.

Invalid env shapes fail as wrapper configuration errors:

- missing `=` is rejected
- empty key is rejected

Verification:
`crates/m80-cli/tests/parse_args.rs::parse_run_visibility_and_exec_options`,
`crates/m80-cli/src/cmds/tests.rs::run_cwd_env_and_stdin_map_to_exec_request`,
`crates/m80-cli/src/cmds/tests.rs::run_env_parser_keeps_empty_values_and_rejects_invalid_shape`,
and `crates/m80-cli/src/cmds/tests.rs::secret_env_requires_named_existing_host_variable`.

## Stdin

`m80 run --stdin -- <program>` reads host stdin fully before backend execution
and sends those bytes as `ExecRequest::stdin`. The guest child receives the
bytes on stdin and then EOF.

When `--stdin` is omitted, the CLI sends no stdin payload and guestd attaches
the child stdin to null.

Verification:
`crates/m80-cli/tests/parse_args.rs::parse_run_visibility_and_exec_options` and
`crates/m80-cli/src/cmds/tests.rs::run_cwd_env_and_stdin_map_to_exec_request`.

## Cwd

`--cwd <path>` is part of the process input contract. It is a guest path and is
forwarded unchanged in the exec request. The CLI does not map it through host
workspace semantics.

Workspace-specific cwd examples should use `/workspace/...`.

Verification:
`crates/m80-cli/src/cmds/tests.rs::run_cwd_env_and_stdin_map_to_exec_request`.

## Terminal Metadata

Pipe mode does not automatically project terminal metadata such as `TERM`,
`LANG`, or `COLORTERM`. Users can pass explicit values with `--env`.

PTY mode will consume this policy instead of redefining it: any automatic
terminal metadata projection must be named in the PTY bead and must stay
separate from host credential or dotfile projection.

## Non-Inputs

The CLI does not automatically inherit host secrets, config files, SSH agents,
package-manager credentials, editor state, or shell startup files. Those belong
to explicit auth/config projection work, not to baseline process input
projection.
