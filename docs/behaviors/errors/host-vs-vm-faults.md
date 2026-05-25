# Host Infrastructure Versus VM Readiness Faults

Behavior capture for `m80-2ggw.7.3`.

## Contract

m80 distinguishes failures before guest readiness into two recovery classes:

- **Host infrastructure fault**: host placement, jailer, or Firecracker-start
  mechanics failed before m80 reached the guest boundary. Examples:
  `FcError::HostInfrastructure { kind, detail }` and
  `FcError::ApiSocketTimeout { path, timeout }`. CLI callers receive
  `EXIT_HOST_INFRASTRUCTURE = 14`.
- **VM readiness fault**: Firecracker started far enough for m80 to wait on the
  guest readiness channel, but `m80-guestd` never connected within the
  readiness budget. This remains
  `FcError::GuestdReadyTimeout { path, timeout }`. CLI callers receive
  `EXIT_GUESTD_READY = 15`.

The structured variants are not collapsed. `ApiSocketTimeout` keeps its path
and timeout fields. `GuestdReadyTimeout` keeps its path and timeout fields.
`HostInfrastructure` carries a finite `HostFaultKind` plus detail for host
faults that are not already represented by a more precise structured variant.

JSON error envelopes keep the stable `variant` field from
`FcError::variant_name()`: `HostInfrastructure`, `ApiSocketTimeout`, or
`GuestdReadyTimeout`.

## Verification

- `crates/m80-firecracker/tests/error_variant_displays.rs` pins
  `HostInfrastructure`, `ApiSocketTimeout`, and `GuestdReadyTimeout`
  classification.
- `crates/m80-cli/src/errors.rs` tests the CLI exit-code and JSON variant
  mappings.
- `crates/m80-cli/src/cmds/warm/control.rs` keeps warm-owner error response
  variants exhaustive over the new `HostInfrastructure` class.
