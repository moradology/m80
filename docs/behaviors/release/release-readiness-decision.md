# Release readiness decision

`scripts/release_readiness_decision.py` merges configured lane receipts into one
release readiness decision. The command takes an explicit stage plus explicit
`lane_id=path` receipt inputs, checks them against
`release-readiness-lanes.json`, and writes one JSON decision for the requested
release tag and commit.

The decision receipt summarizes the underlying lane receipts. It records each
stage, required lane id, lane status, receipt path, artifact paths, digest
field, digest, substrate, and remediation. It does not replace the lane receipts
or their proof artifacts.

Stages keep the release order honest:

- `pre-upload` requires workflow-policy, release-bundle-integrity,
  docs-command, and hostless-quickstart receipts before publish authority or
  upload can run.
- `pre-latest` additionally requires the real-KVM quickstart receipt before
  the workflow can move the release to latest.
- `post-latest-public` adds the unauthenticated public latest receipt after the
  latest pointer has moved.

Required stage lanes must be present and current. The command fails closed for
missing lanes, stale tag or commit, unknown lane state, blocking statuses,
authenticated public-access proof, and fixture substitution for real-substrate
lanes.
