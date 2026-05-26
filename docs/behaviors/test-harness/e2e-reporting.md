# E2E Report Schema

Bead: `m80-16hx7.6`

`scripts/run-e2e.sh --json` emits the privileged E2E batch report. The report
is a fail-closed schema contract, not an informal debug blob:

- `schema_version` is `3`.
- Unknown top-level, environment, artifact, summary, or result fields are
  rejected by `scripts/validate-e2e-report.py`.
- `summary` counts must match the concrete `results` records.
- `skip` results must carry a reason from the ignored-test taxonomy.
- `fail` results must carry stdout and stderr excerpts.
- `exit_code` must be `0` exactly when no failed results are present.

Operator-facing schema and interpretation details live in
[`docs/operations/e2e-reporting.md`](../../operations/e2e-reporting.md).

Regression coverage:

- `python3 scripts/test-run-e2e-report.py`
- `scripts/run-e2e.sh --list --json > /tank/tmp/m80-e2e-report/report.json`
- `python3 scripts/validate-e2e-report.py /tank/tmp/m80-e2e-report/report.json`
