# Privileged E2E Leak Check

`scripts/run-e2e.sh` verifies cleanup after each passing ignored test. The
pre-run reaper removes stale residue from old sessions; this check catches the
different failure mode where a test reports success while leaving m80-owned
host state behind.

## What It Checks

Before and after a test exits `0`, the wrapper runs:

```sh
scripts/e2e-reap.sh --dry-run --json --run-root "$M80_E2E_RUN_ROOT" --min-age-hours 0
```

`scripts/e2e-leak-diff.py` compares those dry-run reports. Any newly reported
cleanup action or newly skipped live resource becomes a test failure with
`reason: "leak-check"` in the JSON report. Reaper errors become `reason:
"leak-check-error"`. The diff report is stored in that result's
`stdout_excerpt`, so CI can show the leaked resource kind and target without
blaming residue that existed before this test started.

The check covers the same resource classes as the reaper:

- m80-owned TAP/bridge/link names;
- m80-comment-owned iptables rules;
- empty m80 per-VM iptables chains;
- direct child run directories under the selected run root;
- empty cgroup v2 leaves under `/sys/fs/cgroup/m80-firecracker`.

## Cascade Prevention

When a leak is detected, the wrapper records the failure and then runs the
real reaper with `--min-age-hours 0` for the selected run root. That keeps the
next test from inheriting residue from the failed cleanup assertion. Set
`M80_E2E_SKIP_REAPER=1` only when preserving leaked state for manual debugging.

## Opt Out

Set `M80_E2E_SKIP_LEAK_CHECK=1` to disable post-test leak verification while
debugging the leak checker itself. Do not use this for CI or release evidence.

## Library Helper

`m80-test-helpers::leak_check` provides pure snapshot/diff helpers used by
unit tests. The wrapper uses `scripts/e2e-reap.sh --dry-run` at runtime so the
post-test check and pre-run cleanup share one live ownership classifier.
