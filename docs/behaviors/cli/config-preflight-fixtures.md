# CLI Config And Preflight Fixtures

Behavior capture for bead `m80-lt15.3`.

## Contract

CLI-level config and preflight behavior must be testable without requiring the
developer machine to have KVM, Firecracker artifacts, `/etc/m80/config.toml`,
`/etc/m80/config.d`, or real `~/.config/m80` state.

The tests isolate config sources before asserting behavior:

- `load_config_from_paths` receives explicit `ConfigFilePaths` with
  `system: None`, `system_drop_in_dir: None`, and user paths under the temp
  home.
- `HOME` points at the same temporary directory for any code path that still
  observes it.
- Recognized `M80_*` variables are removed or set by the fixture.
- The host's real `/etc/m80/config.toml` and `/etc/m80/config.d` are not read.

## Covered Paths

Config precedence:

- user config file under isolated `$HOME/.config/m80/config.toml`
- user config.d directory under isolated `$HOME/.config/m80/config.d`
- `M80_*` environment variables overriding the user config
- caller flag overrides overriding `M80_*`
- built-in defaults when both config path layers are skipped
- unknown config keys failing closed instead of being ignored

CLI rendering and preflight:

- config human table rendering from an in-memory `EffectiveConfig`
- config JSON rendering from an in-memory `EffectiveConfig`
- preflight success rendering from an in-memory `Discovery`
- preflight failure mapping through `crates/m80-cli/src/errors.rs`

## Test Files

- `crates/m80-cli/tests/config_preflight_fixtures.rs`
- unit tests in `crates/m80-cli/src/cmds.rs`
- `crates/m80-cli/tests/facade_runner.rs`

No test in this set reads the developer's real `~/.config/m80/config.toml` or
`~/.config/m80/config.d`.
Any test that relies on real host preflight must live in a separate ignored KVM
lane.
