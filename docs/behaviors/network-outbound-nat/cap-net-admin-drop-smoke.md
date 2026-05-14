# Outbound NAT After Parent Capability Drop

Behavior capture for bead `m80-8emae.34.5`.

`m80-firecracker` starts the pinned `m80-net-helper` during backend
initialization and then drops `CAP_NET_ADMIN` from the backend thread.
`AllowOutbound` launch, host policy application, normal delete cleanup, and
stale run-root cleanup must continue to work through the helper after that
thread no longer has `CAP_NET_ADMIN` in `CapEff`, `CapPrm`, or `CapBnd`.

The privileged smoke test is
`crates/m80-firecracker/tests/egress_outbound_real_kvm.rs::allow_outbound_survives_parent_cap_net_admin_drop`.
It requires a prepared KVM host and `M80_RUN_EXTERNAL_NETWORK_E2E=1`. The test
constructs a backend, checks `/proc/thread-self/status` for the capability
drop, boots an `AllowOutbound` VM, proves external DNS and HTTP-by-IP work,
deletes that VM, then boots and stops a second outbound VM without delete so
`recover_stale_run_root(true)` must clean the stale run-dir through the helper.
