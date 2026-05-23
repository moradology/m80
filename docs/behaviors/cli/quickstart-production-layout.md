# Quickstart Production Layout

`m80 quickstart` keeps the user-facing flow in
`crates/m80-cli/src/cmds/quickstart.rs` and moves helper behavior into focused
private modules under `crates/m80-cli/src/cmds/quickstart/`:

- `artifact.rs` owns required artifact names, external checksum verification,
  and bundled-host-prerequisite rejection.
- `release_url.rs` owns public GitHub release URL identity checks and repair
  diagnostics.
- `provenance.rs` owns installed manifest/build-receipt rewrites and
  `install-provenance.json` emission.
- `profile_writer.rs` owns installed default profile and config writes.
- `probe.rs` owns the generated host-binaries manifest for the runnable probe
  and the exact `m80 run -- echo hello` probe command.
- `process.rs` and `temp_tree.rs` own subprocess/error wrapping and temporary
  extraction cleanup.

The split is private to the CLI crate. It does not add compatibility shims,
alternate quickstart paths, or new public commands.

Verification:

- `crates/m80-cli/src/cmds/quickstart/probe.rs::tests::echo_probe_command_is_plain_public_target`
- `crates/m80-cli/src/cmds/quickstart/probe.rs::tests::echo_probe_scrubs_runtime_env_overrides`
- `crates/m80-cli/src/cmds/quickstart/release_url.rs` unit tests for public
  release URL matching, latest rejection, dev-build diagnostics, and local
  override behavior.
- `crates/m80-cli/tests/quickstart_smoke/` covers checksum verification,
  forbidden bundle contents, installed profile/config generation, rollback,
  host-manifest status, and probe preflight gating.
