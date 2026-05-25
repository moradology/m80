# Outbound NAT TAP MTU

Behavior capture for `m80-2ggw.3.4`.

## Contract

Outbound NAT creates the Firecracker TAP inside the private VMM network
namespace. The private topology plan carries `tap_mtu: Option<u32>`:

- `None` preserves the kernel default TAP MTU and emits no MTU operation.
- `Some(mtu)` sets the TAP MTU after the TAP is brought up inside the VMM
  namespace.
- Accepted values are `576..=9000`; out-of-range values fail with
  `NetError::InvalidTapMtu` before topology mutation.

Current public `OutboundIntent` and `SandboxConfig` callers do not expose this
knob. The launch path populates `None` so existing outbound NAT launches keep
their current kernel-default TAP MTU behavior. A later policy/API leaf can
surface the setting without changing the lower topology semantics.

## Verification

`crates/m80-net-outbound/tests/network-outbound-nat/setup.rs` pins the behavior:

- `bridge_setup_is_idempotent_with_matching_state` proves the default setup path
  emits no `set_link_mtu*` operation.
- `private_netns_tap_topology_sets_optional_tap_mtu_after_tap_up` proves
  `tap_mtu=Some(9000)` emits the namespaced TAP MTU operation after TAP link-up.
- `private_netns_tap_topology_rejects_out_of_range_mtu_before_mutation` proves
  values outside `576..=9000` fail before link or namespace mutation.
