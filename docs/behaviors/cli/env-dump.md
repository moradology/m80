# CLI Environment Dump

Behavior capture for bead `m80-81rw`.

## Contract

`m80 env` produces the diagnostic bundle operators need when filing a bug. It
does not launch a VM and does not require KVM to succeed. Host preflight is
reported as data rather than used as command authority.

`m80 --json env` emits the standard CLI JSON envelope with `data.version: 1`.
The payload includes:

- `cli_version` and `protocol_version`
- host kernel version, CPU count, KVM device accessibility, vsock module
  evidence, virtualization CPU flags, and total memory
- effective config when config loading succeeds
- selected runtime profile and profile artifact paths when resolvable
- kernel/rootfs/kernel-kind artifact paths from profile or `M80_*`
- rootfs manifest path and parse status
- Firecracker binary path, version subprocess output when discoverable, and
  configured version pin
- run-root path, existence, and VM directory count
- preflight status and check rows when full preflight succeeds

Human output is the same diagnostic surface rendered as compact key/value text.

## Bug Reports

The issue template asks reporters to attach:

```sh
m80 --json env > m80-env.json
```

For failure-specific context, reporters can also attach:

```sh
m80 --json logs <vm-id> > m80-logs.json
```

## Verification

- `crates/m80-cli/src/cmds/env.rs` tests that the JSON dump has the standard
  envelope, a payload version, and bug-report sections without requiring KVM.
- `crates/m80-cli/tests/parse_args.rs` pins the `m80 env` parse surface.
- `crates/m80-cli/tests/help_smoke.rs` pins the visible help surface.
