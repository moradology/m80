# Freshness Workflow Wiring

`.github/workflows/latest-freshness.yml` is the scheduled hostless guard for
the public install surface. It runs on a cron schedule and by manual dispatch,
checks out the repository, and invokes `scripts/release_freshness.py` against
the public latest release and docs command inventory.

The workflow is deliberately read-only. Top-level and job permissions grant
only `contents: read`; there is no release upload, release edit, release
delete, or broader GitHub write API step. The job runs on `ubuntu-latest`
instead of a self-hosted or real-KVM runner because this lane proves public URL
freshness and hostless install metadata only. Privileged public quickstart proof
belongs to the real-KVM freshness lane.

Freshness runs are serialized by the `latest-freshness-public` concurrency
group with `cancel-in-progress: false`. A newer scheduled run must not cancel
an older run while that older run is still collecting evidence. The workflow
pre-creates a freshness proof placeholder before checkout, then overwrites it
with `scripts/release_freshness.py --proof-out` on success. If the verifier
fails before writing a proof, the workflow writes a small failure envelope so
the artifact upload still has a machine-readable handle.

The evidence upload uses `if: always()` and includes:

- `m80-latest-freshness-proof.json`;
- `m80-latest-freshness.stdout`;
- `m80-latest-freshness.stderr`.

`scripts/lint-github-workflows.py` treats freshness workflows as release-guarded
workflows and additionally requires schedule/manual triggers, read-only job
permissions through the normal permission checks, non-canceling concurrency,
the `scripts/release_freshness.py` invocation, hostless runner shape, no release
mutation commands, and an always-uploaded proof/log artifact.
