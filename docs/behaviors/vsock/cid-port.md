# Vsock CID and Port Allocation

## derivation

Each VM's vsock guest CID is derived deterministically from `vm_id` via
SHA-256: the first four bytes of the digest are read as a big-endian `u32`,
then mapped into the range `3..=u32::MAX - 1` using
`3 + (raw % (u32::MAX - 3))`. The same `vm_id` always produces the same CID
across restarts; no allocation table is needed.

**Implementation:** `m80-vsock::cid_for_vm_id` in
`crates/m80-vsock/src/lib.rs`.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:41-53`
(`guest_cid_for_vm_id`, which used FNV-1a; m80 uses SHA-256 for the same
determinism guarantee).

## reserved-range

The derivation modulus ensures the produced CID is always in the range
`3..=u32::MAX - 1`, keeping it outside the reserved vsock CIDs:
`VMADDR_CID_ANY = u32::MAX`, `VMADDR_CID_HOST = 2`, `VMADDR_CID_HYPERVISOR = 1`,
and the "any" address 0. The formula `3 + (raw % (u32::MAX - 3))` guarantees
the maximum output is `3 + (u32::MAX - 4) = u32::MAX - 1`, and the minimum is
`3`.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:47-52`
(FC-CORR-5 comment) and test `guestd_for_vm_uses_stable_non_reserved_cid`.

## fixed-port

The guest daemon listens on fixed vsock port `9001` (`GUEST_PORT_DEFAULT`).
The host always dials this port without per-VM port discovery. The constant is
re-exported from `m80-vsock` so callers can reference it symbolically.

The host bridge UDS lives at `<run_dir>/vsock.sock`. `Channel::open` receives
the full path; it does not assume any layout above the path argument.

**Predecessor source:** `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:15`
(`DEFAULT_GUESTD_VSOCK_PORT = 9001`).
