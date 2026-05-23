# Install Cleanup

`m80 install-cleanup --release-tag <tag>` removes one installed version
directory from `<install-root>/versions/`. It is for reclaiming disk space after
an upgrade or after a manual rollback has moved the active pointer to another
verified version.

The command only accepts a single release-tag directory name. It refuses path
separators, parent traversal, symlinked install/version directories, mixed
ownership across the required install-tree entries, and version directories
that do not look like m80 install output. The candidate must be one direct child
of the configured install root's `versions/` directory and must contain the
expected top-level `bundle.json`, `bin/m80`, and `artifacts/` entries.

By default, cleanup refuses to remove the active version. Operators should
rollback first by switching `<install-root>/active` to a previous verified
version, then remove the now-inactive version. `--remove-active` is the explicit
break-glass path: it unlinks the active pointer before removing that version
directory, leaving no active installed release selected.

Regression coverage lives in `crates/m80-cli/tests/release/install_cleanup.rs`.
It covers inactive cleanup, active refusal, explicit active removal, cleanup
after rollback, malformed version directory refusal, and release-tag path escape
rejection under an install-root fixture.
