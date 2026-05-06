# Non-Release Conditions

## Generic Conditions

m80's generic VM layer names three cleanup outcomes that must prevent a caller
from treating the local sandbox as safely releasable:

- forced kill outcome is ambiguous
- owned host cleanup failed
- owned residue may still represent a live VM

These are VM-mechanics blockers only. Agent writeback, placement leases,
idempotency keys, and commit authority live outside m80. The backend surfaces
typed errors and lifecycle evidence; the caller decides what those facts mean
for its own placement state.

Test:
- `crates/m80-firecracker/tests/cleanup/teardown_phase_order.rs::generic_block_set`
