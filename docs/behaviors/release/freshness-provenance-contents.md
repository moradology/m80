# Freshness Provenance Contents

The latest freshness verifier does not treat downloaded provenance files as
opaque assets. After URL, checksum, and asset-index checks pass, it parses:

- `m80-release-integrity.json`;
- `m80-release-integrity.attestation.jsonl`.

The predicate must match the resolved latest stable tag, `moradology/m80`, the
selected default Linux bundle, and the selected bundle metadata sidecar. The
freshness proof records the predicate digest, attestation bundle digest,
subject names, subject digests, release tag, and source URL for each checked
value.

The attestation bundle must be nonempty JSONL. Each entry must be a Sigstore
bundle carrying a DSSE in-toto statement. The statement must identify the
release workflow tag ref for the same resolved tag and must attest the
`m80-release-integrity.json` digest observed from public release metadata.

Failures are classified as `provenance-mismatch` and include the role, asset,
URL, release tag, field, expected value, observed value, and repair command.
Fixture coverage rejects wrong bundle digest, wrong metadata digest, wrong
release tag, missing subject, malformed predicate JSON, and malformed
attestation JSONL.
