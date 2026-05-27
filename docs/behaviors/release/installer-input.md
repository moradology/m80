# Installer Input Trust Model

Bead: `m80-o3uh9.15.11.7`

The normal public Linux install path is the release `install.sh`, either through
the stable latest channel or a concrete pinned tag. The public docs must keep
those paths as the first-run surface:

Public installer status: pending until the unauthenticated public-access proof
is green.

- latest stable release: `curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh`
- pinned release: `curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh | sudo sh`
- already-installed verifier: `m80 install --release-tag <tag>`

Direct bundle URLs are lower-level install inputs. They are explicit operator,
fixture, or repair inputs, not the public latest selector. The accepted official
shape is a concrete m80 release asset URL such as
`m80 install --bundle-url https://github.com/moradology/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz`.
For that shape, the CLI verifies the same-tag public integrity material before
it lists or extracts the tarball, creates staging state, writes profile state,
or switches the active pointer.

## Official Bundle URLs

An official direct bundle URL must bind to all of these before bytes are
trusted:

- repository `moradology/m80`;
- concrete stable tag from the URL;
- expected Linux x86_64 bundle asset name;
- same-tag asset index row verified by release integrity;
- bundle bytes verified by release integrity and the asset index row;
- metadata sidecar verified by release integrity;
- `install.sh` verified by release integrity;
- `m80-bootstrap-selector.tsv` verified by release integrity;
- `m80-release-build.json` verified by release integrity;
- `m80-release-integrity.json`;
- `m80-release-integrity.attestation.jsonl`;
- `m80-release-attestation.json`;
- the GitHub Artifact Attestation policy rooted in the trusted verifier
  distribution.

The direct URL path is therefore not a checksum-only compatibility mode for
official `moradology/m80` release assets. It is the same release-integrity
contract as the public installer, entered from a more explicit operator input.

## Overrides And Rejections

Local `file://` bundles and local fixture HTTP URLs are operator/test overrides.
They may exercise installer layout code, but they do not claim official release
trust and do not fetch public attestation material.

Rejected shapes include:

- mutable latest bundle artifacts such as the
  `releases/latest/download/<bundle>.tar.gz` shape;
- foreign GitHub release assets such as
  `https://github.com/example/m80/releases/download/v1.2.3/m80-linux-x86_64.tar.gz`;
- raw branch installer URLs;
- non-HTTPS GitHub release URLs;
- non-bundle release assets such as `install.sh`;
- path-traversal asset names.

Those shapes fail before network access or install-root mutation when they can
be rejected from the URL alone.

## Troubleshooting

Official direct URL verifier failures are no-write failures. The diagnostic
names the resolved release tag when known, the failed material class, the
expected and observed identity or digest, and exactly one safe retry command for
the explicit operator URL. See
[`direct-url-diagnostics.md`](direct-url-diagnostics.md) for the finite JSON
codes, success fields, and retry-command contract.

The broader source-selection and dry-run contract remains in
[`installer-input-contract.md`](installer-input-contract.md).
