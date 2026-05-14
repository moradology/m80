# CLI Auth And Config Projection

Behavior capture for bead `m80-lt15.17.4`.

## No Implicit Credentials

`m80 run` does not inherit host credentials, dotfiles, SSH agents, package
manager config, editor state, or shell startup files by default. Anything an
interactive tool needs must be named explicitly.

This keeps the process-wrapper model honest: a sandboxed process has a limited
view of the host, not a best-effort clone of the user's login session.

## Secret Environment

`--secret-env KEY` copies exactly one named host environment variable into the
guest child environment. The key must be present on the host. The key must be a
plain variable name, not `KEY=VAL`.

The value crosses the host/guest vsock channel inside `ExecRequest.env`.
`M80_DEBUG_WIRE=vsock` redacts that field in trace previews; other diagnostic
targets and ordinary application output are still caller/operator
responsibility.

This is the preferred current path for token-style credentials:

```text
m80 run --workspace . --egress outbound --secret-env ANTHROPIC_API_KEY -- claude
```

Verification:
`crates/m80-cli/tests/parse_args.rs::parse_run_visibility_and_exec_options` and
`crates/m80-cli/src/cmds/tests.rs::secret_env_requires_named_existing_host_variable`.

## Literal Environment

`--env KEY=VAL` remains available for non-secret or already-materialized values.
It is explicit projection too, but the value appears in argv/shell history, so
docs should prefer `--secret-env` for credentials.

Verification:
`crates/m80-cli/src/cmds/tests.rs::run_env_parser_keeps_empty_values_and_rejects_invalid_shape`.

## Config File Mounts

`--mount-config <host>:<guest>[:ro]` is a reserved shape and exits with
feature-gap code 7 before backend work starts. Config-file projection needs a
separate mount/hydration design so m80 can validate host paths, guest targets,
read-only behavior, and cleanup without growing a broad host-filesystem escape.

Verification:
`crates/m80-cli/tests/feature_gap_smoke.rs::run_mount_config_is_explicit_feature_gap`.

## Claude-Shaped Recipe

The current explicit recipe for an interactive coding tool is:

```text
m80 run -it --workspace . --egress outbound --secret-env ANTHROPIC_API_KEY -- claude
```

`-it` requests a guest terminal and live host terminal input. The visibility,
auth, and network rules are still explicit: the token is named by
`--secret-env`, the workspace is named by `--workspace`, and outbound egress is
named by `--egress outbound`.

## Non-Goals

- No automatic host dotfile mounts.
- No broad `$HOME` projection.
- No SSH agent forwarding until a dedicated policy names it.
- No agent tool catalog or `EffectClass` semantics in m80 core.
