# Freshness Drift Diagnostics

Behavior bead: `m80-o3uh9.21.1.7`.

The public latest freshness verifier separates semantic drift from transient
network failures. Timeout, DNS/connect, redirect-loop, partial-download, curl
spawn, and metadata-fetch failures are classified as `network-transient`.
Public release problems that need operator repair use distinct classes:

- `docs-drift` for stale public docs snippets or stale generated docs status;
- `missing-public-asset` for required release assets that are absent or 404;
- `stale-latest` for tag agreement and latest guard mismatches;
- `checksum-mismatch` for digest disagreement across public metadata,
  checksum files, asset-index rows, or proof digests;
- `provenance-mismatch` for URL identity, size, predicate, attestation, and
  trust-policy disagreement;
- `public-release-unavailable` when GitHub latest is not an eligible stable
  public release;
- `real-kvm-substrate-unavailable` when the privileged proof runner cannot
  supply the documented KVM substrate;
- `verifier-schema-drift` when verifier inputs are malformed enough that the
  checker cannot trust them.

Each failure prints one compact stderr summary with `failure_class`, failing
source path or URL, release tag when known, expected value, observed value, and
the first repair command from
`docs/behaviors/release/freshness-failure-policy.json`. The JSON proof carries
the same information under `failure.details`: parsed fields, source,
`release_tag`, `expected`, and `observed`.

The proof intentionally keeps the compact stderr message and the structured JSON
derived from the same text. Tests cover every verifier-emitted failure class so
future changes cannot add a human-only or machine-only diagnostic path.
