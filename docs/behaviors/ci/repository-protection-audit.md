# Repository Protection Audit

Behavior capture for bead `m80-o3uh9.13.6.1`.

## Contract

Release publication depends on repository settings that live outside the git
tree. The tag publish job must prove those settings are still present before it
uploads release assets, publishes a draft, or marks a release as latest.

`scripts/repository_protection_audit.py` writes
`m80-repository-protection-audit.json` with:

- `schema_version: 1`
- `kind: m80_repository_protection_audit`
- repository, branch, release tag pattern, and publish environment
- one check each for main-branch required status checks, an active `v*` tag
  ruleset, and `m80-release-publish` required reviewers
- structured expected, observed, status, and remediation fields for every check

The audit exits non-zero for missing, weak, unreadable, or unauthenticated
settings. The release workflow runs it before the publish authority receipt and
uploads the JSON audit as `m80-repository-protection-audit-<run id>` on green
runs. Failed publish diagnostics also preserve the audit file when it exists.

## Verification

`scripts/test-repository-protection-audit.py` covers protected fixtures, missing
main-branch required checks, missing release-tag rulesets, missing environment
approval, and unavailable GitHub API payloads.

`scripts/lint-github-workflows.py` requires the release publish job to run the
audit against `m80-release-publish`, preserve the audit JSON in the publish
scratch directory, and upload the named audit artifact.
