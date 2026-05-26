# E2E Leak Check

Bead: `m80-16hx7.5`

The privileged E2E wrapper treats cleanup as part of a passing test's contract.
For each ignored test, `scripts/run-e2e.sh` captures a before and after report
from `scripts/e2e-reap.sh --dry-run --json --run-root <run-root>
--min-age-hours 0`, then diffs them with `scripts/e2e-leak-diff.py`.

Contract:

- no newly reported reaper actions or skipped live resources means the test
  result remains `pass`;
- any new action or skipped live resource turns the test result into `fail` with reason
  `leak-check`;
- any dry-run reaper error turns the test result into `fail` with reason
  `leak-check-error`;
- after recording the failure, the wrapper invokes the real reaper to prevent
  leak cascades into later tests.

Regression coverage:

- `cargo test -p m80-test-helpers --test leak_check`
- `python3 scripts/test-e2e-leak-diff.py`
- `bash -n scripts/run-e2e.sh scripts/e2e-reap.sh`
- `shellcheck -s bash scripts/run-e2e.sh scripts/e2e-reap.sh`
