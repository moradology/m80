# Freshness Repair Commands

Behavior bead: `m80-o3uh9.21.5.3`.

Freshness failures use
`docs/behaviors/release/freshness-failure-policy.json` as the repair catalog.
Each configured failure class has one `repair_command`, a disposition, and an
owner. `scripts/release_freshness.py` appends the cataloged repair command to
failure diagnostics as `repair_command=...`, so operators and repair beads do
not need to infer the next action from prose.

The catalog is intentionally small:

- transient network failures rerun the bounded hostless freshness verifier;
- stale latest and missing public assets point at the release repair beads;
- docs drift points at the generated freshness-status check;
- checksum and provenance mismatches point at release-integrity verification;
- real-KVM substrate failures point at the real-KVM release smoke epic;
- schema drift reruns the failure-policy verifier.

Commands that mutate installs or release assets must be version-pinned and must
not target mutable latest or raw `main`. CI validates the policy, verifies that
every verifier-emitted class has a catalog row, and checks that the runbook's
catalog table matches the JSON source of truth.
