# Installer PATH Handoff

Behavior bead: `m80-o3uh9.3.4`.

Successful release installs publish an `m80` command in the configured bin
directory and verify that the invoking shell resolves that command before the
active pointer is flipped.

## Bin Directory

The default bin directory is `/usr/local/bin` when installing into `/opt/m80`.
For explicit fixture or proof install roots, the default is
`<install-root>/bin`. Operators may override it with `m80 install --bin-dir
<PATH>`.

The handoff path is always:

```text
<bin-dir>/m80 -> <install-root>/versions/<release_tag>/bin/m80
```

The installer uses a symlink so the command path names the selected release
binary directly; it does not wrap or dispatch through mutable install logic.

## Verification

Before flipping `<install-root>/active`, the installer:

- links `<bin-dir>/m80` to the staged release binary;
- verifies `command -v m80` resolves to `<bin-dir>/m80`;
- runs `<bin-dir>/m80 --version`;
- prints `installed_m80_path`, `installed_m80_version`, `release_tag`,
  `active_bundle_path`, and `next_command=m80 run -- echo hello`.

If an older `m80` earlier in `PATH` shadows the configured bin directory, the
install fails before changing the active pointer and prints an exact repair
command:

```text
export PATH=<bin-dir>:$PATH
```

The handoff is transactional for the command link: if the `command -v` or
`m80 --version` check fails, the installer restores the previous
`<bin-dir>/m80` link, or removes the newly created link when there was no prior
one.

## Tests

- `install_bundle_layout_copies_verified_bundle_into_version_dir`
- `install_bundle_layout_fails_when_older_m80_shadows_installed_path`
- `install_bundle_layout_fails_with_repair_when_m80_is_not_on_path`
- `install_bundle_layout_rejects_existing_command_directory_before_handoff_mutation`
- `install_bundle_layout_explicit_bin_dir_override_controls_handoff_path`
