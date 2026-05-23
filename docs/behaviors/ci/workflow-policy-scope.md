# Workflow Policy Scope

Behavior capture for bead `m80-o3uh9.13.36`.

## Contract

Every GitHub workflow has an explicit m80 policy scope in
`docs/behaviors/ci/workflow-policy-scope.json`.

Scopes:

- `ordinary-ci`: normal CI checks. It still gets global workflow linting, but
  it does not receive release-only guards such as release concurrency,
  release job timeouts, release cargo lock checks, or release toolchain pin
  checks.
- `release-authority`: tag-release build or publish authority. It receives the
  release guard set and any release-artifacts-specific publish authority checks
  when it owns those jobs.
- `latest-freshness`: scheduled/manual public latest freshness checks. It
  receives the release guard set plus freshness-specific hostless and artifact
  publication checks.
- `proof`: proof-producing release workflow. It receives the release guard set.

The filename is only a backstop for fail-closed diagnostics. A file named like a
guarded workflow (`release`, `latest`, `freshness`, `proof`, or `publish`) is
not allowed to appear without a policy entry, but the configured scope is the
source of truth for which guards apply.

## Enforcement

`scripts/lint-github-workflows.py` reads the policy config before linting
workflow YAML. It rejects a missing config, unknown schema version, unknown
fields, duplicate workflow entries, unknown scope names, configured files that
do not exist, and workflow files that are not represented in the policy.

The linter applies release guards from `release-authority`,
`latest-freshness`, and `proof`, not from filename tokens. It applies the
freshness-specific checks only to `latest-freshness`.

## Verification

`scripts/test-workflow-policy.py` covers:

- a renamed release workflow that still receives release guards because the
  policy marks it `release-authority`;
- a guarded-looking filename without a policy entry;
- an `ordinary-ci` workflow that is not forced through release-only guards;
- duplicate policy entries;
- unknown workflow scopes;
- configured workflow files that are missing.
