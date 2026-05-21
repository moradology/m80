# Release Publish Authority Policy

`scripts/release_publish_authority.py` is the runtime gate for GitHub release
mutation. It runs inside the release workflow's publish job before the publish
decision receipt and before `gh release upload`.

The checked policy is intentionally narrow:

- repository: `moradology/m80`
- workflow path: `.github/workflows/release-artifacts.yml`
- publish job id: `publish-release-artifacts`
- build job id: `build-release-artifacts`
- allowed ref shape: `refs/tags/v*`
- token source: `github.token`
- top-level permissions: `contents: read`
- build job write permissions: `id-token: write`, `attestations: write`
- publish job write permissions: `contents: write`

The gate rejects branch refs, wrong repositories, wrong workflow refs, wrong job
ids, missing or unexpected token source, missing token material, missing release
jobs, missing job permissions, publish jobs without a tag guard, and any write
permission outside the expected build-attestation and publish-upload jobs.

This is separate from `scripts/lint-github-workflows.py`. The linter catches
static workflow drift in CI. The publish authority gate re-checks the same
release boundary at the mutation point with the actual GitHub context variables
that own release upload authority.

Regression coverage lives in `scripts/test-workflow-policy.py` under the
`test_publish_authority_policy_*` fixtures.
