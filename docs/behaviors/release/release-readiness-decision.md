# Release readiness decision

`scripts/release_readiness_decision.py` merges configured lane receipts into one
publish decision. The command takes explicit `lane_id=path` receipt inputs,
checks them against `release-readiness-lanes.json`, and writes one JSON decision
for the requested release tag and commit.

The decision receipt summarizes the underlying lane receipts. It records each
lane status, receipt path, artifact paths, digest field, digest, substrate, and
remediation. It does not replace the lane receipts or their proof artifacts.

Required publish-blocking lanes must be present and current. The command fails
closed for missing lanes, stale tag or commit, unknown lane state, blocking
statuses, authenticated public-access proof, and fixture substitution for
real-substrate lanes.
