# Transparent Hugepage Advisory

Bead: `m80-jp6ik.28`.

`m80-preflight` surfaces the host transparent hugepage policy from
`/sys/kernel/mm/transparent_hugepage/enabled` as an informational report row.
The check is advisory-only: it never changes preflight pass/fail status.

The row behavior is:

- `[always]` reports a clean row.
- `[madvise]` reports a passing row with an advisory that Firecracker guest
  memory will not receive THP unless the child process explicitly madvises it.
- `[never]` reports a passing row with an advisory that THP is disabled for
  guest memory.
- unreadable or unrecognized policy text reports a passing row that says the
  advisory could not be evaluated.

The operator-facing tuning guidance is in `docs/ops/host-tuning.md`.

Pinned tests:

- `thp_always_policy_reports_clean_row`
- `thp_madvise_policy_reports_advisory`
- `thp_never_policy_reports_advisory`
- `thp_unreadable_policy_is_non_blocking`
- `thp_malformed_policy_is_non_blocking`
