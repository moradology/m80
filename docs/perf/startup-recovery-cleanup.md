# Startup Recovery Network Cleanup

Date: 2026-05-14

Bead: `m80-jp6ik.15`

## Verification

Unit coverage for the stale-run-dir recovery path:

```sh
cargo test -p m80-firecracker backend::tests::remove_run_dir --lib
```

Result:

- `remove_run_dir_cleans_network_before_deleting_state`: passed
- `remove_run_dir_still_reaps_when_network_cleanup_fails`: passed

Real host outbound smoke:

```sh
sudo env \
  M80_RUN_EXTERNAL_NETWORK_E2E=1 \
  M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
  M80_JAILER_BIN=/opt/firecracker/bin/jailer \
  M80_JAILER_HARDEN_BIN=/tank/projects/m80/target/release/m80-jailer-harden \
  M80_FIRECRACKER_VERSION=v1.15.1 \
  M80_KERNEL_IMAGE=/tmp/m80-build/minimal/vmlinux \
  M80_ROOTFS_IMAGE=/tmp/m80-build/minimal/output.ext4 \
  M80_RUN_ROOT=/var/lib/m80-run \
  cargo test -p m80-firecracker --test egress_outbound_real_kvm \
    allow_outbound_resolves_external_dns -- --ignored --exact --nocapture
```

Result: passed twice on the prepared real-KVM host.

The second run bracketed the smoke with `iptables-save` before and after. The
m80-owned iptables delta was zero:

```text
m80-ish added 0
m80-ish removed 0
before m80-ish 176
after m80-ish 176
```

Post-run link inspection found no leftover `tfc*`, `brfc*`, or `m80` links:

```sh
ip link show | rg -n "tfc|brfc|m80" || true
```

## Interpretation

The real outbound path can create, use, and normally tear down TAP/iptables
state without adding new host residue. The unit tests pin the startup recovery
ordering that matters for crashed runs: if `network-state.json` exists,
`remove_run_dir` calls `m80_net_outbound::cleanup_vm(vm_id, run_root)` before
deleting the run directory that contains that state file.

There is pre-existing m80 iptables residue on this host (`176` m80-ish
iptables-save lines before the second smoke). Because the before/after delta
was zero, the residue was not introduced by this verification run. It remains a
separate cleanup/orphan-scan concern; this bead verifies that the implemented
normal and stale-run-dir cleanup paths do not create new residue in the tested
outbound flow.
