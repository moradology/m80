Release mismatch diagnostics keep first-run failures actionable. A user with
only the `m80` binary should not have to infer the missing release bundle from
`kernel image not found`, `rootfs image not found`, or an unrelated host
preflight failure.

## No Installed Profile

`m80 run -- echo hello` fails before backend or host-preflight work when the
selected runtime profile is the built-in `env` profile and none of
`M80_KERNEL_IMAGE`, `M80_ROOTFS_IMAGE`, or `M80_ARTIFACT_DIR` is set. Human
stderr names `no installed default profile`; JSON stderr uses the shared error
envelope with `variant: "Config"` and `code: "no_installed_profile"`.

Release builds print both repair forms:

```text
current version: curl -fsSL https://github.com/moradology/m80/releases/download/<tag>/install.sh | sudo sh
latest stable: curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh
```

Dev builds must not suggest mutable public latest. They print the explicit
local/operator path instead:

```text
dev build: create a local bundle for <version>-dev and run m80 install --bundle-url file:///path/to/m80-linux-x86_64.tar.gz
```

Explicit artifact input remains authoritative. If the caller sets
`M80_KERNEL_IMAGE` and `M80_ROOTFS_IMAGE`, or sets `M80_ARTIFACT_DIR`, `m80 run`
continues to normal profile-aware preflight.

## Stale Installed Profile

An install-owned default profile with missing referenced paths fails before
unrelated host checks. The diagnostic names the missing profile fields and the
same install repair command for the running binary identity.

Regression coverage:

- `crates/m80-cli/src/cmds/tests.rs::run_default_env_profile_without_artifacts_prints_install_repair`
- `crates/m80-cli/src/cmds/tests.rs::run_default_env_profile_with_artifact_env_is_allowed_to_preflight`
- `crates/m80-cli/src/cmds/tests.rs::release_no_profile_repair_names_current_and_latest_installer`
- `crates/m80-cli/src/cmds/tests.rs::stale_installed_profile_paths_print_reinstall_command`
- `crates/m80-cli/tests/output_error_contract.rs::run_without_installed_profile_reports_repair_code_before_preflight`
