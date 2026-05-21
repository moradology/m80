# Freshness Public Assets

The hostless freshness verifier checks the public release metadata before it
fetches installer URLs. URL liveness is not enough: the latest release must
publish the complete installer-consumed asset set with stable names, GitHub
download URLs, sizes when GitHub reports them, and SHA-256 digests from the
GitHub release asset metadata.

`scripts/release_freshness.py` treats these assets as required for a stable
latest install:

- `install.sh` and `install.sh.sha256`;
- `m80-release-assets.json` and `m80-release-assets.json.sha256`;
- `m80-bootstrap-selector.tsv` and `m80-bootstrap-selector.tsv.sha256`;
- the default Linux bundle, bundle metadata sidecar, and their checksum
  sidecars;
- `m80-release-build.json` and its checksum sidecar;
- `m80-release-integrity.json`;
- `m80-release-integrity.attestation.jsonl`;
- `m80-release-attestation.json`;
- `SHA256SUMS`.

Each asset row in the freshness proof records `name`, `role`, `url`,
`release_tag`, `size_bytes`, and `sha256`. Proof rows come from the validated
release asset set, so `install.sh` keeps the canonical `installer` role even
when checked through latest, pinned, and docs-linked URLs. The role vocabulary
is intentionally small: installer, asset-index, selector, provenance,
attestation, bundle, checksum, metadata, or public-asset. Missing assets fail
before URL fetches and name the role, asset, expected public URL, and release
tag.

When a release asset-index fixture is supplied, freshness also compares the
GitHub metadata digest and size for the default bundle and bundle metadata
against the asset index row. Digest or size drift fails closed with expected
and observed values. Malformed asset-index fixtures name the role, asset,
public URL, release tag, and malformed field so the repair target is obvious.

This lane still does not prove the bytes execute. It proves that the public
latest release exposes a complete, self-describing install input set whose
release metadata agrees with the indexed bundle identity. Hostless install-root
verification and real-KVM quickstart proof are separate freshness leaves.
