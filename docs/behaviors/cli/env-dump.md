# CLI Environment Dump

Behavior capture for bead `m80-81rw`.

## Contract

`m80 env` produces the host/config diagnostic dump used by the bug-report
bundle. It does not launch a VM and does not require KVM to succeed. Host
preflight is reported as data rather than used as command authority.

`m80 --json env` emits the standard CLI JSON envelope with `data.version: 1`.
The payload includes:

- `cli_version` and `protocol_version`
- host kernel version, CPU count, KVM device accessibility, vsock module
  evidence, virtualization CPU flags, and total memory
- effective config when config loading succeeds
- selected runtime profile name, selection source, body source, profile file,
  profile artifact paths, host-helper paths, run-root field, release tag, and
  m80 version when resolvable
- installed-profile diagnostics: active pointer path, target, `live`/`stale`/
  `missing`/`error` status when the artifact path is under an install-root
  `versions/<tag>/artifacts` tree, and a field-by-field list of missing
  profile paths
- kernel/rootfs/kernel-kind artifact paths from profile or `M80_*`
- rootfs manifest path and parse status
- Firecracker binary path from the selected profile or `M80_*`, version
  subprocess output when discoverable, and configured version pin
- run-root path, existence, and VM directory count
- preflight status and check rows when full preflight succeeds

Human output is the same diagnostic surface rendered as compact key/value text.

## Bug Reports

The public issue template asks reporters to attach the composed, redacted
support bundle:

```sh
m80 bug-report > m80-bug-report.json
```

For failure-specific VM context, reporters pass `--vm-id <vm-id>` to the same
command. See [`bug-report.md`](bug-report.md).

## Verification

- `crates/m80-cli/src/cmds/env/tests.rs` tests that the JSON dump has the standard
  envelope, a payload version, and bug-report sections without requiring KVM.
- `crates/m80-cli/src/cmds/env/tests.rs::env_json_reports_selected_installed_profile_paths`
  proves selected profile paths are reported and preferred over ambient
  artifact/helper environment variables.
- `crates/m80-cli/src/profile/report.rs` tests prove active install pointers
  are classified as `live`, `stale`, or `missing`, and that missing profile
  paths are named by field.
- `crates/m80-cli/tests/parse_args.rs` pins the `m80 env` parse surface.
- `crates/m80-cli/tests/help_smoke.rs` pins the visible help surface.
