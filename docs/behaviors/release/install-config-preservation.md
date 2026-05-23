# Install Config Preservation

`m80 install` writes the installed product selector files only when it can do so
without silently erasing operator choices. For the default install root those
files are `/etc/m80/config.toml` and `/etc/m80/profiles/default.toml`; for an
install-root fixture they are `<install-root>/config.toml` and
`<install-root>/profiles/default.toml`.

The generated selector is owned by m80. A fresh install writes it, and an
install with byte-identical existing selector files may proceed. If either file
already exists with different contents, install fails before active state is
changed. The error names the existing path, the proposed generated path, the
exact `m80 install ... --adopt-existing-config` command for hard cutover, and a
`cp -a` backup command.

`--adopt-existing-config` is an explicit hard cutover. It replaces the old
selector with the generated installed selector; it does not merge old config
keys or keep a compatibility matrix. If finalization later fails after adoption,
the previous config/profile bytes are restored before the command exits.

Regression coverage lives in
`crates/m80-cli/tests/release/installer_layout/config_preservation.rs`. It covers
fresh/matching selector writes, conflicting config/profile refusal, explicit
adoption, and rollback after adoption followed by a late PATH handoff failure.
