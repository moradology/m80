# Quickstart Proof Artifacts

m80 uses one JSON proof shape for hostless release fixtures, real-KVM release
smokes, and scheduled freshness checks. The schema is enforced by
`scripts/verify-quickstart-proof.py`; producers may differ, but they must emit
the same fields before a quickstart lane is considered green.

The v1 proof records:

- `release`: requested selector, resolved concrete release tag, and install URL.
- `command`: display command, argv, expected exit status, observed exit status,
  and whether the command is the expected-nonzero companion smoke.
- `stream_expectations`: stdout and stderr markers the validator checks to
  catch stream swaps and missing captures.
- `stdout` plus `stderr`: stdout excerpt and either a stderr excerpt or a
  relative stderr artifact path.
- `install`: install root, active pointer, and default profile path.
- `m80`: version, release tag, and version status from the installed binary.
- `bundle`: relative bundle metadata path plus release tag, m80 version, guest
  protocol version, and manifest schema version.
- `host_binaries`: relative `host-binaries.manifest.json` path plus
  Firecracker and jailer versions.
- `substrate`: `hostless` or `real-kvm` plus a summary. Hostless fixtures must
  say they are not real-KVM run-smoke proof.

The validator checks that relative artifact paths exist under the uploaded
proof root, that bundle metadata and host-binary manifest references are
present, that the resolved tag matches the expected release tag, that observed
exit status equals expected exit status, and that expected stdout/stderr markers
appear on the correct streams. It rejects unknown fields so fixture, release,
and freshness producers cannot silently drift.

Tracker close discipline is guarded separately by
`scripts/verify-release-tracker-policy.py`. Open `m80-o3uh9` proof-shaped beads
and their parents carry `requires-verified-close`, and closed labeled leaves
must cite `verified: <artifact-path> @ <commit-sha>` for a committed proof
artifact containing command, stdout/stderr or log path, exit status, resolved
tag, and substrate. See
[`verified-close-policy.md`](verified-close-policy.md).

The tag release workflow writes
`m80-quickstart-proof-hostless.json` into the `m80-release-dist` GitHub Actions
artifact and validates it before upload. The publish job validates the same
proof again after downloading the workflow artifact. Real-KVM release smoke and
latest freshness jobs must upload their own proof JSON with `proof_kind:
"real-kvm"` and run the same validator before marking quickstart proof green.
The expected-nonzero companion smoke should use `expected_nonzero: true` and
matching nonzero expected/observed exit statuses to prove process-wrapper
exit-code passthrough.

Inspect a proof artifact with:

```sh
scripts/verify-quickstart-proof.py \
  m80-quickstart-proof-hostless.json \
  --artifact-root /path/to/downloaded/m80-release-dist \
  --release-tag vX.Y.Z
```

Relevant tests:

- `scripts/test-quickstart-proof.py`
- `scripts/test-release-tracker-policy.py`
- `scripts/test-release-bundle.py::test_release_workflow_publishes_and_verifies_proof_assets`
