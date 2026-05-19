# CLI Image/Profile Selection

Behavior capture for bead `m80-lt15.18`.

## Contract

`m80 run` selects a local runtime profile before host preflight discovers boot
artifacts. A profile is a label for an already-built m80 guest image, not an OCI
image reference, package-install recipe, or workload DSL.

Default behavior is the built-in profile named `env`. It preserves the current
artifact discovery contract:

- `M80_KERNEL_IMAGE` supplies the guest kernel, or preflight searches
  `M80_ARTIFACT_DIR`.
- `M80_ROOTFS_IMAGE` supplies the ext4 rootfs.
- the manifest beside the rootfs supplies `image_kind`, hashes, guest port, and
  kernel kind unless `M80_KERNEL_KIND` explicitly overrides it.

Callers can select a named profile with:

```text
m80 run --profile dev -- python -V
```

If `--profile` is absent, the `default_profile` config field is used. Its
built-in value is `env`; it can be set in config files, with
`M80_DEFAULT_PROFILE`, or by the `--profile` flag for one run.
`m80 quickstart` sets the host default to the installed profile named `default`;
that generated profile is described in
`docs/behaviors/cli/installed-default-profile.md`.

## Profile Files

Named profiles are TOML files searched in this order:

1. `/etc/m80/profiles/<name>.toml`
2. `~/.config/m80/profiles/<name>.toml`

The user file wins when both exist. The name must be a single path segment; path
traversal and absolute path spelling fail as config errors.

Profile TOML schema:

```toml
artifact_dir = "/opt/m80/images/python" # optional
kernel_image = "/opt/m80/images/python/vmlinux-stripped"
rootfs_image = "/opt/m80/images/python/rootfs.ext4"
kernel_kind = "stripped" # optional: stock|stripped
guestd = "/opt/m80/images/python/m80-guestd" # optional
guest_manifest = "/opt/m80/images/python/output.ext4.manifest.json" # optional
build_receipt = "/opt/m80/images/python/output.ext4.build-receipt.json" # optional
install_provenance = "/opt/m80/images/python/install-provenance.json" # optional
host_binaries_manifest = "/opt/m80/images/python/host-binaries.manifest.json" # optional
firecracker_bin = "/opt/firecracker/bin/firecracker" # optional
firecracker_seccomp_filter = "/opt/firecracker/bin/firecracker-seccomp-filter.bin" # optional
jailer_bin = "/opt/firecracker/bin/jailer" # optional
jailer_harden_bin = "/opt/m80/bin/m80-jailer-harden" # optional
net_helper_bin = "/opt/m80/bin/m80-net-helper" # optional
run_root = "/var/run/m80" # optional metadata; installed config supplies runtime run_root
release_tag = "v0.1.0" # optional
m80_version = "v0.1.0" # optional
description = "Python tool image" # optional operator-facing text
```

All profile paths must be absolute host paths. Unknown TOML keys fail closed.
`m80` does not create missing files, pull images, or silently fall back to
another profile.

During backend construction, the resolved profile overlays the boot-artifact
and host-helper environment variables for the preflight call:

- `M80_ARTIFACT_DIR`
- `M80_KERNEL_IMAGE`
- `M80_ROOTFS_IMAGE`
- `M80_KERNEL_KIND` when present in the profile
- `M80_FIRECRACKER_BIN`
- `M80_FIRECRACKER_SECCOMP_FILTER`
- `M80_JAILER_BIN`
- `M80_JAILER_HARDEN_BIN`
- `M80_NET_HELPER_BIN`

The overlay is process-local and restored after preflight. This is the fixture
proof that selected profiles reach the same boot-artifact resolution path as
environment-driven launches.

## Config Visibility

`m80 config show` renders the merged effective config without running KVM or
artifact preflight. The output includes `default_profile` and its source label,
so operators can see whether the selected default came from the built-in value,
system config, user config, env, or a caller-supplied flag path.

## Runtime Availability

Commands such as `claude`, `python`, `node`, or `bash` must already exist in the
selected guest profile or inside the visible workspace. m80 does not execute
host binaries with the same name and does not install missing programs.

## Evidence

- `crates/m80-cli/src/profile.rs` unit tests pin default `env` resolution,
  named profile search precedence, fail-closed parsing, and env overlay.
- `crates/m80-firecracker/tests/config_loading.rs` pins `default_profile`
  defaults, env mapping, and flag precedence.
- `crates/m80-cli/tests/parse_args.rs::parse_run_runtime_profile_shape` pins the
  CLI flag shape.
