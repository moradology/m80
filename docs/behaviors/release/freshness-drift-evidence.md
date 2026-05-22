# Freshness Drift Evidence

Behavior bead: `m80-o3uh9.21.5.1`.

The scheduled public freshness workflow writes
`m80-latest-freshness-drift.json` next to stdout, stderr, and the normal
freshness proof. This artifact is the first debugging input for repair beads:
it preserves the failure class, source URL or docs snippet, resolved/latest tag
context, expected asset or digest, observed status or value, verifier version,
workflow run id, and a bounded redacted stderr excerpt.

The schema is versioned as `schema_version: 1` and validated by:

```sh
python3 scripts/freshness_drift_evidence.py --validate <artifact>
```

`status: success` is allowed so the workflow can upload deterministic JSON even
when no drift occurred. `status: failure` requires a configured freshness
failure class and a concrete `source` object:

- `source.kind: url` for public release metadata or asset URL failures;
- `source.kind: file` plus `snippet_id` for docs-command inventory drift.

Repair automation should read this artifact before rerunning the freshness
check. The stderr excerpt is intentionally small and must not contain token,
password, GitHub PAT, AWS key, bearer-token, or private-key shaped values.
