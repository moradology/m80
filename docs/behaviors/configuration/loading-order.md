# Configuration Loading Order

## Contract

The canonical loader in `m80-firecracker` resolves backend configuration in this
order:

1. Built-in defaults.
2. System config file: `/etc/m80/config.toml`.
3. System config drop-ins: `/etc/m80/config.d/*.toml`, lexicographic order.
4. User config file: `~/.config/m80/config.toml`.
5. User config drop-ins: `~/.config/m80/config.d/*.toml`, lexicographic order.
6. Recognized `M80_*` environment variables.
7. Caller-supplied CLI flag overrides.

Later layers override earlier layers per key. A user config file can override
only `run_root` while leaving `max_concurrent_vms` from a system drop-in, and
an environment variable can then override either file layer.

Only files with a `.toml` extension are loaded from config.d directories.
Missing config.d directories are ignored. A config.d path that exists but is
not a directory fails closed. Parse errors and unknown keys in any loaded
drop-in file fail closed.

## Recognized Keys

Config files and flag overrides accept exactly these top-level keys:

- `default_profile`
- `max_concurrent_vms`
- `run_root`
- `jail_uid`
- `jail_gid`
- `cgroup_mode`

Unknown keys fail closed with `FcError::Config`. This is intentional: m80 does
not treat config typos as future extension points.

## Effective Config

The loader returns `EffectiveConfig`, a sorted list of fields tagged with the
winning `ConfigSource`:

- `default`
- `system_file`
- `system_drop_in`
- `user_file`
- `user_drop_in`
- `env`
- `flag`

`m80 config show` renders this same effective config so operators can see which
layer supplied each value. Walk-only CLI commands such as `m80 list` and
`m80 inspect` resolve `run_root` through the same loader but skip preflight, so
they can inspect residue on machines where KVM or artifacts are currently
unavailable.

`default_profile` is included in the effective config. The CLI reads it before
preflight so runtime profile selection can choose the kernel/rootfs artifact
inputs. The built-in value is `env`, meaning "use the existing preflight
environment/default artifact discovery path."

## Hermetic Fixtures

Production loading uses host paths. Tests and embedders that need deterministic
behavior use:

```rust
load_config_from_paths(
    flags,
    ConfigFilePaths {
        system: Some(system_path),
        system_drop_in_dir: Some(system_config_d),
        user: Some(user_path),
        user_drop_in_dir: Some(user_config_d),
    },
)
```

`None` skips that file layer. This keeps tests from accidentally reading the
developer's real `/etc/m80/config.toml` or home config.

## Evidence

- `crates/m80-firecracker/tests/config_loading.rs::built_in_defaults_are_loaded_without_host_files`
  pins defaults.
- `crates/m80-firecracker/tests/config_loading.rs::precedence_is_defaults_system_user_env_flags`
  pins the full precedence chain.
- `crates/m80-firecracker/tests/config_loading.rs::system_drop_ins_apply_after_system_file_in_lexicographic_order`
  pins drop-in ordering.
- `crates/m80-firecracker/tests/config_loading.rs::user_drop_ins_override_system_drop_ins_and_user_file`
  pins drop-in source precedence.
- `crates/m80-firecracker/tests/config_loading.rs::drop_in_unknown_key_fails_closed`
  and `::drop_in_parse_error_fails_closed` pin fail-closed drop-in behavior.
- `crates/m80-firecracker/tests/config_loading.rs::user_file_overrides_system_file_per_key`
  pins per-key file layering.
- `crates/m80-cli/tests/config_preflight_fixtures.rs` covers the CLI-facing
  fixture path and `m80 config show` render helpers without KVM.
