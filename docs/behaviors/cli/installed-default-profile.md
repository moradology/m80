# Installed Default Profile

Behavior capture for bead `m80-o3uh9.9.2`.

## Contract

`m80 quickstart` makes the installed guest bundle the default runtime target.
After a successful install, the normal follow-up command is:

```sh
m80 run -- echo hello
```

No `M80_KERNEL_IMAGE`, `M80_ROOTFS_IMAGE`, or `M80_ARTIFACT_DIR` export is part
of the common path.

Quickstart writes two host files after the artifact tarball checksum, extracted
`SHA256SUMS`, required artifact presence, install-path relocation, and install
provenance write have all succeeded:

- `/etc/m80/profiles/default.toml`
- `/etc/m80/config.toml`

Install-root fixture overrides can direct those writes to temporary paths, but
the runnable probe uses the host config/profile locations.
The runnable probe starts its child `m80 run -- echo hello` with known m80
artifact, profile, helper, config, preflight-cache, and trace environment
overrides removed so the installed host files are what the probe exercises.

## Generated Profile

`default.toml` records final absolute host paths only:

- `artifact_dir`
- `kernel_image`
- `rootfs_image`
- `kernel_kind`
- `guestd`
- `guest_manifest`
- `build_receipt`
- `install_provenance`
- `host_binaries_manifest`
- `firecracker_bin`
- `firecracker_seccomp_filter`
- `jailer_bin`
- `jailer_harden_bin`
- `net_helper_bin`
- `run_root`
- `release_tag`, when the artifact URL contains `/releases/download/<tag>/`
- `m80_version`
- `description`

The host-binaries manifest path is recorded even in `--no-run` mode; that mode
does not run host substrate verification, does not generate the live host
manifest, and does not claim real-KVM proof.

## Generated Config

Quickstart updates the config root table with:

```toml
default_profile = "default"
run_root = "/var/run/m80"
```

Existing recognized config keys are preserved. Unknown config keys still fail
closed through the normal config loader.

## Runtime Use

`m80 run` resolves `default_profile` from the effective config, loads the named
profile, and overlays its artifact and host-helper paths for preflight before
building the backend. The overlay is restored after backend construction. The
installed config supplies the default run-root.

Operator overrides keep their normal precedence:

- `--profile <name>` selects a profile for one run.
- `M80_DEFAULT_PROFILE` or user config can select a different default profile.
- `--profile env` or `M80_DEFAULT_PROFILE=env` selects the built-in env
  profile, where artifact/helper `M80_*` values drive discovery directly.
- A named profile supplies every field it records as a process-local discovery
  value. Ambient `M80_*` values still fill any optional helper path the selected
  profile omits.

## Failure And Rollback

The profile/config write is transactional across the two files. If config write
fails after the profile was written, quickstart restores the previous profile
and config contents where they existed. Missing files are removed again.

## Evidence

- `crates/m80-cli/tests/quickstart_smoke.rs::quickstart_no_run_installs_verified_artifacts`
  proves install-root profile/config creation and generated fields.
- `crates/m80-cli/tests/quickstart_smoke.rs::quickstart_profile_records_release_tag_from_download_url`
  proves release tag capture.
- `crates/m80-cli/tests/quickstart_smoke.rs::quickstart_overwrites_stale_profile_and_config`
  proves stale file overwrite.
- `crates/m80-cli/tests/quickstart_smoke.rs::quickstart_rolls_back_previous_profile_when_config_write_fails`
  proves rollback after a failed second write.
- `crates/m80-cli/src/profile.rs` unit tests prove profile path validation and
  artifact/helper env overlay.
- `crates/m80-cli/src/cmds/quickstart.rs` unit tests pin the m80 environment
  overrides removed from the runnable probe.
