# Freshness Status

The latest freshness lane uses a small status JSON for humans, docs, and
release gates. The status is not the proof itself. It is a pointer to the last
validated public-latest proof, plus enough identity fields to prove that the
docs command inventory, GitHub latest tag, pinned installer URL, source commit,
and proof artifacts all agree.

`scripts/verify-freshness-status.py` validates schema version `1`. The top-level
fields are exact:

- `schema_version`: currently `1`.
- `generated_at`: UTC timestamp with second precision.
- `status`: `pending`, `public_green`, `scaffolded`, `failed`, or `stale`.
- `owner` and `repo`: the GitHub release root from
  `docs/behaviors/release/public-release-root.env`.
- `resolved_latest_tag`: concrete stable tag returned by the latest path.
- `expected_highest_stable_tag`: highest stable tag the freshness lane expected.
- `latest_install_url`: the public latest `install.sh` URL.
- `pinned_install_url`: the same install script under `releases/download/<tag>`.
- `proof_artifacts`: relative proof refs with `kind`, `path`, `sha256`, and
  `artifact_class`.
- `proof_substrate`: `public-unauthenticated`, `fixture-hostless`, `real-kvm`,
  or `none`.
- `workflow_run_id`: the workflow run that produced or validated the status.
- `source_commit`: the 40-character source commit for the verifier checkout.
- `checked_command_inventory_digest`: SHA-256 of the public command inventory
  from README/crate/runbook/behavior docs.
- `install_url_proofs`: latest and pinned installer URL proof rows.
- `public_assets`: sorted public asset rows from the release metadata view.

`pending` is the docs-safe state before public proof exists. It uses
`proof_substrate: none` and empty proof artifact, install URL proof, and public
asset lists. `public_green` is intentionally strict. It requires public
unauthenticated proof for both the latest install URL and the pinned install
URL, `http_status: 200` for both rows, matching resolved and expected stable
tags, public proof artifact classes, and the complete sorted required public
asset set. Fixture-only proof may appear only as `scaffolded`; it cannot be
relabeled as public proof.

The command inventory digest is recomputed from the checked-out docs. Any manual
edit to a public quickstart block, pinned command, status command, or legacy
quickstart reference changes the digest and makes old freshness status stale.
This is the handoff point for generated README/runbook status rendering:
`scripts/render-freshness-status.py` updates only the explicit
`m80:freshness-status` marker blocks in README and the release runbook. The
latest install command can be present while the rendered status remains
`pending`; the command is the release-channel template, and the status is the
proof state for treating that template as currently public-proven.

Retention is append-friendly: keep the latest status JSON and its referenced
proof artifacts together under one artifact root. Failed or stale runs should
publish their own failure artifact without overwriting the last `public_green`
status. The follow-on stale-green guard owns preserving and comparing the
previous passing status.

Schema and verifier coverage for this contract is captured in
`docs/behaviors/release/freshness-status-schema-proof.json`.
