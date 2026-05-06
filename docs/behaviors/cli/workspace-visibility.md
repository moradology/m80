# CLI Workspace Visibility

Behavior capture for bead `m80-lt15.17.1`.

## Workspace Flag

`m80 run --workspace <dir> -- <program>` exposes exactly one caller-selected
host tree to the guest process. The CLI carries the path into
`SandboxConfig::workspace`; lower storage code hydrates that tree into the
per-VM workspace scratch image.

Inside the guest, the workspace is mounted at `/workspace`. The selected host
directory is not bind-mounted directly into the VM, and no other host path
becomes visible because it is adjacent to the workspace on the host.

Verification:
`crates/m80-cli/tests/parse_args.rs::parse_run_visibility_and_exec_options` and
`crates/m80-cli/src/cmds/tests.rs::run_workspace_and_scratch_map_to_sandbox_config`.

## No Workspace

When `--workspace` is omitted, `SandboxConfig::workspace` is `None`; no
workspace scratch drive is attached. The guest image may still contain a
`/workspace` directory, but it is not populated from the host and should not be
treated as a host-visible tree.

Verification:
`crates/m80-cli/tests/parse_args.rs::parse_run_defaults_to_process_wrapper_contract`
and
`crates/m80-cli/src/cmds/tests.rs::run_defaults_to_no_workspace_and_default_overlay_size`.

## Cwd

`--cwd <path>` is a guest process path. The CLI forwards it into the exec
request as-is; it is not interpreted as a host path and it is not joined to the
host workspace path.

Invalid or missing guest cwd paths fail in the guest execution path. The CLI
does not probe the guest filesystem during argument parsing.

Verification:
`crates/m80-cli/tests/parse_args.rs::parse_run_visibility_and_exec_options` and
`crates/m80-cli/src/cmds/tests.rs::run_cwd_env_and_stdin_map_to_exec_request`.

## Workspace Contents

Workspace hydration admits directories and regular files. Symlinks and special
files are rejected by `m80-storage` during scratch creation; the CLI does not
try to paper over that failure or broaden the visible host surface.

Rootfs overlay writes and workspace scratch writes are separate effects.
Workspace visibility does not imply rootfs writeback, host dotfile projection,
agent workspace policy, or any `EffectClass` semantics.
