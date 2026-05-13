# KVM Halt-Poll Advisory

Bead: `m80-jp6ik.30`.

`m80-preflight` surfaces KVM halt-poll settings as an informational report row.
The check is advisory-only: it never changes preflight pass/fail status.

The row includes `/sys/module/kvm/parameters/halt_poll_ns` when available. It
also includes `halt_poll_ns_grow`, `halt_poll_ns_shrink`,
`lapic_timer_advance`, and Intel `enable_preemption_timer` when those files
exist on the host.

If `halt_poll_ns` is unavailable, preflight still passes and reports that the
advisory could not be evaluated.

The operator-facing tuning guidance is in `docs/ops/host-tuning.md`.

Pinned tests:

- `kvm_halt_poll_reports_current_value_as_advisory`
- `kvm_halt_poll_reports_timer_interaction_when_available`
- `kvm_halt_poll_unavailable_is_non_blocking`
