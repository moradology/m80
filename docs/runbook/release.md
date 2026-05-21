# Release Runbook

This runbook owns the release identity contract used by the installer and
quickstart flow.

## Version Source

Release builds inject the GitHub release tag, source commit, and Rust target
triple at compile time:

```sh
M80_RELEASE_TAG=vX.Y.Z \
  M80_RELEASE_COMMIT=<40-hex-commit> \
  M80_RELEASE_TARGET_TRIPLE=<rust-target-triple> \
  cargo build --release -p m80-cli
```

The injected tag must be the exact `v<workspace package version>` tag. For the
workspace package version `0.0.0`, the expected release tag is `v0.0.0`.
The injected source commit must be the exact commit used by the release build
manifest and signed release integrity material.
The injected Rust target triple must be one of the target triples recorded in
the release build manifest.

`m80 --version` and `m80 version` expose the release identity:

- dev builds render as `<package-version>-dev`;
- release builds render the injected GitHub release tag;
- `m80 --json version` includes `package_version`, `release_tag`,
  `release_build`, `version_status`, `expected_release_tag`,
  `source_commit`, `target`, `target_triple`, `protocol_version`,
  `manifest_schema_version`, `build_receipt_schema_version`, and
  `install_provenance_schema_version`.

Packaging must refuse to publish a bundle when `version_status` is `dev` or
`mismatch`, when `source_commit` is missing or differs from the release manifest
commit, or when the binary target identity does not match the release target
and build manifest target triples. The installer and quickstart resolver must
not use `releases/latest` from a dev build; dev builds require an explicit local
bundle or artifact URL.

## Public Install URLs

The public GitHub release owner/repo source is
`docs/behaviors/release/public-release-root.env`. The README and this runbook
must use snippets rendered by `scripts/render-release-install-snippets.py`:

Public installer status is pending until the unauthenticated public-access
proof is green for the promoted release. The latest snippet is the
release-channel template, not a current-success claim, until that proof is
green.
<!-- m80:public-access-proof m80-o3uh9.21.7 pending -->

<!-- m80:quickstart-snippet latest-install start -->
```sh
curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh
```
<!-- m80:quickstart-snippet latest-install end -->

<!-- m80:quickstart-snippet pinned-install start -->
```sh
curl -fsSL https://github.com/moradology/m80/releases/download/<version>/install.sh | sudo sh
```
<!-- m80:quickstart-snippet pinned-install end -->

Automation from a trusted checkout uses the verified handoff block when it must
prove `install.sh` before sudo:

<!-- m80:quickstart-snippet verified-install-handoff start -->
```sh
tag=<version>
repo=moradology/m80
tmp="$(mktemp -d)"
base="https://github.com/${repo}/releases/download/${tag}"
for asset in install.sh install.sh.sha256 m80-release-integrity.json m80-release-integrity.attestation.jsonl m80-release-attestation.json; do
  curl -fsSLo "${tmp}/${asset}" "${base}/${asset}"
done
python3 scripts/verify-install-handoff.py "${tmp}" \
  --release-tag "${tag}" \
  --trust-policy docs/behaviors/release/m80-release-trust-policy.json \
  --verification-time "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
sudo sh "${tmp}/install.sh"
```
<!-- m80:quickstart-snippet verified-install-handoff end -->

Do not hand-write alternate owners, raw `main` URLs, or private checkout URLs
for public install instructions.

The common latest and pinned snippets are stable-channel only. The resolved
release must be public, non-draft, non-prerelease, tagged exactly
`vMAJOR.MINOR.PATCH`, must publish the complete installer-consumed asset set,
and must have a green unauthenticated public-access proof before docs treat it
as promoted. `scripts/stable_release_channel.py` validates GitHub release
metadata and the asset index for future latest bootstrap/freshness lanes.
`scripts/stable_latest_bootstrap.py` resolves latest to one concrete stable tag,
checks that latest has not switched before handoff, and emits pinned URLs for
`install.sh`, the bundle, checksum sidecars, asset index, bootstrap selector,
release integrity, attestation, and public checksum material. `install.sh`,
`m80 install --release-tag`, and packaging also reject prerelease-shaped tags
before network/index work. See
`docs/behaviors/release/stable-channel.md`.

Latest metadata resolution is bounded separately from release-asset downloads:
both the initial and guard GitHub metadata fetches use a 10 second connect
timeout, 120 second total timeout, two retries, and a one second retry delay
before emitting install handoff JSON. URL-mode failures name the fetch role,
metadata URL, and curl failure class. Fixture mode stays network-free and does
not require curl.

The shell installer bounds every release-asset download before the bundled
`m80 install` binary can take over. Each fetch uses a 10 second connect timeout,
120 second total timeout, two retries, and a one second retry delay. Failure
diagnostics name the release tag, asset, URL, curl failure class, and whether
release verification had started. Offline or private-network hosts are expected
to fail clearly; this policy is not an offline install guarantee.

`scripts/verify-install-handoff.py` verifies downloaded `install.sh` bytes and
their signed release-integrity subject before automation runs local verified
bytes with `sudo`. It prints the release tag, source commit,
`install_sh_sha256`, and verified asset names before the privilege-sensitive
handoff. See
`docs/behaviors/release/verified-install-handoff.md`.

## Verification

The release identity is pinned by:

- `crates/m80-cli/src/release.rs` unit tests for dev, release, and mismatch
  identity;
- `crates/m80-cli/tests/version_smoke.rs` for `m80 --version` and JSON
  `m80 version` output.

## Bundle Contract

The Linux bundle contract is documented in
`docs/behaviors/release/bundle-contract.md`. Build the bundle from already-built
inputs with:

```sh
M80_RELEASE_COMMIT="$(git rev-list -n 1 "$M80_RELEASE_TAG")"

scripts/package-release-bundle.py \
  --release-tag "$M80_RELEASE_TAG" \
  --commit-sha "$M80_RELEASE_COMMIT" \
  --rust-toolchain 1.82 \
  --target-triple "$(rustc +1.82 -vV | awk '/^host:/ {print $2}')" \
  --target-triple x86_64-unknown-linux-musl \
  --builder-identity "local-release-builder:$(hostname)" \
  --builder-os-image "$(uname -sr)" \
  --apt-package-version busybox-static="$(dpkg-query -W -f='${Version}' busybox-static)" \
  --apt-package-version musl-tools="$(dpkg-query -W -f='${Version}' musl-tools)" \
  --apt-package-version e2fsprogs="$(dpkg-query -W -f='${Version}' e2fsprogs)" \
  --apt-package-version curl="$(dpkg-query -W -f='${Version}' curl)" \
  --target linux-x86_64 \
  --image-kind minimal \
  --m80-bin target/release/m80 \
  --jailer-harden-bin target/release/m80-jailer-harden \
  --net-helper-bin target/release/m80-net-helper \
  --kernel /tmp/m80-release-artifacts/vmlinux \
  --rootfs /tmp/m80-release-artifacts/output.ext4 \
  --rootfs-manifest /tmp/m80-release-artifacts/output.ext4.manifest.json \
  --build-receipt /tmp/m80-release-artifacts/output.ext4.build-receipt.json \
  --guestd /tmp/m80-release-artifacts/m80-guestd \
  --install-sh scripts/install.sh \
  --out-dir /tmp/m80-release-bundle
```

This package command is the compatibility gate before upload. It reads
`m80 --json version`, `m80-guestd --version`, the guest manifest, and the build
receipt; then it refuses schema drift, protocol drift, wrong target/image kind,
stale release identity, or manifest/receipt hashes that do not match the
supplied artifacts.

Do not add official Firecracker, official jailer, or Firecracker seccomp filter
payloads to the bundle. v0.x policy treats those bytes as operator-provided host
prerequisites, so both the bundle verifier and `m80 quickstart` reject them
before an install can become active. If m80 ever starts installing a pinned
Firecracker train itself, file a new policy decision and hard-cutover bead
instead of making the current verifier tolerant.

`scripts/install.sh` is the versioned installer template. Packaging renders it
with only the concrete release tag, publishes it as `install.sh`, and embeds the
same rendered file inside the release bundle. The script downloads the pinned
release's bootstrap selector and canonical asset index, verifies their checksum
sidecars, selects the matching host tuple from the selector, then downloads the
release integrity predicate, attestation bundle, normalized attestation
metadata, installer asset, metadata sidecar, public checksum manifest, and the
selected bundle. After verifying the signed predicate, attestation signer,
selected bundle checksum, and size, it extracts that bundle's `bin/m80` and
checks that `m80 --json version` agrees with the verified release tag, source
commit, target, target triple, protocol, schema, and bundle metadata, then hands
off to `m80 install --bundle-url file://...`.
It must not carry a rendered per-tuple bundle URL, call `scripts/quickstart.sh`,
or use the legacy artifact-only quickstart flow.

Verify a produced bundle before upload:

```sh
scripts/verify-release-bundle.py \
  /tmp/m80-release-bundle/m80-linux-x86_64.tar.gz \
  --release-tag "$M80_RELEASE_TAG" \
  --verify-sidecars
```

The package step emits a deterministic tarball, checksum sidecars, an
inspectable `m80-linux-x86_64.bundle.json` metadata sidecar, a canonical
`m80-release-assets.json` asset index, a shell-safe
`m80-bootstrap-selector.tsv` projection of that index for the no-installed-binary
installer path, `m80-release-build.json` for build-input provenance, and a
public `SHA256SUMS` covering those assets plus their checksum sidecars. Extra
tuple manifests add their own bundle, metadata sidecar, and checksum sidecars;
the package command derives the asset index, bootstrap selector, public
checksum manifest, and integrity predicate from that assembled row set. The
exact builder contract is
captured in `docs/behaviors/release/bundle-builder.md`.

The complete installer/bootstrapper-consumed subject set recorded inside
`m80-release-integrity.json` for the signed/attested one-tuple default Linux
dist is:

```text
m80-linux-x86_64.tar.gz
m80-linux-x86_64.tar.gz.sha256
install.sh
install.sh.sha256
m80-linux-x86_64.bundle.json
m80-linux-x86_64.bundle.json.sha256
m80-release-assets.json
m80-release-assets.json.sha256
m80-bootstrap-selector.tsv
m80-bootstrap-selector.tsv.sha256
m80-release-build.json
m80-release-build.json.sha256
SHA256SUMS
```

Multi-tuple releases add each extra row's bundle, bundle checksum sidecar,
metadata sidecar, and metadata checksum sidecar to that same subject set and to
public `SHA256SUMS`. The current assembler emits no detached signature files.
The asset index, selector, public `SHA256SUMS`, and release-integrity predicate
prove the public dist rows. Full tar contract verification is separate:
`scripts/verify-release-bundle.py --verify-sidecars` reopens every bundle named
by the asset index and checks its internal `bundle.json`, guest manifest, build
receipt, and bundled `SHA256SUMS` before publish.

## Asset Index

The release asset index is the machine-readable selector for public bundles.
It lists every bundle by OS, architecture, image kind, release tag, m80 version,
guest protocol, manifest schema, expected Firecracker version, tarball digest,
metadata digest, and integrity-material references. The default Linux
quickstart tuple is `linux` / `x86_64` / `minimal`. The published file is
`m80-release-assets.json`; its checksum sidecar and the public `SHA256SUMS`
cover the index before the publish job re-downloads and validates it. The
release workflow also publishes the integrity predicate and attestation bundle
used by the verifier. The asset-index `signature_name` field is nullable in
schema v1; official signed release rows require `attestation_name:
m80-release-integrity.attestation.jsonl`.

The bootstrap selector is generated from the asset index, not maintained by
hand. It exists so `install.sh` can select a bundle with POSIX shell tooling
before a local `m80` binary is available. Verification compares the selector
back to `m80-release-assets.json`; changing one without regenerating the other
is a release-blocking drift.

Adding a new architecture or image kind is a release-contract change, but it is
not a quickstart-command change. Use this checklist:

1. Package the new tuple artifact and its metadata sidecar with a unique
   bundle name.
2. Write a tuple manifest with `schema_version: 1`, `bundle_path`,
   `metadata_path`, `bundle_name`, and `metadata_name`, then pass it to
   `scripts/package-release-bundle.py --extra-tuple-manifest`.
3. Let the package command generate `m80-release-assets.json`,
   `m80-bootstrap-selector.tsv`, their checksum sidecars, public `SHA256SUMS`,
   and `m80-release-integrity.json`; do not edit generated JSON or TSV by hand.
4. Before publishing or promoting the multi-row index, run
   `python3 scripts/verify-release-bundle.py <default-bundle> --release-tag <tag> --verify-sidecars`
   against the exact dist directory so every asset-index row's tar internals are
   checked.
5. Then run
   `python3 scripts/verify-release-integrity.py ... --dist-dir <release-dist>`
   against the exact dist directory.
6. Extend the release proof fixture for the new tuple or image kind before
   publishing it as supported.
7. Update docs only where the supported tuple matrix or image-kind behavior
   changes. The common README latest and pinned install snippets stay:
   `curl .../install.sh | sudo sh`.

## Release Integrity Material

The release workflow uses the schema in
`docs/behaviors/release/release-integrity-material.md`. The public mechanism is
GitHub Artifact Attestations over `m80-release-integrity.json`; that predicate
records the release tag, commit SHA, target, Rust toolchain, m80 package
version, bundle metadata hash, build manifest identity, and the sha256/size of
every current public installer/bootstrapper-consumed dist asset. The same
verifier also loads
`docs/behaviors/release/m80-release-trust-policy.json`,
`m80-release-integrity.attestation.jsonl`, and
`m80-release-attestation.json` so human verification and installer verification
share one trust-anchor path.

Prerequisites for the signed v1 verifier: `python3`, plus a selected GitHub CLI
that includes `gh attestation verify` with `--repo`, `--bundle`,
`--signer-workflow`, `--cert-oidc-issuer`, `--source-ref`,
`--source-digest`, `--deny-self-hosted-runners`, and `--format`. The verifier
checks this before reading release proof material, and the installer checks it
before downloading official release assets or touching active install state. If
the `gh` check fails: Install or upgrade GitHub CLI with attestation support
from <https://cli.github.com/packages>.

Verify a downloaded dist directory before treating a release as signed. This
one read-only command verifies the bundle tar contract, bundle checksum,
installer checksum, metadata sidecar, public sidecars, signed predicate,
attestation metadata, cryptographic attestation bundle, and tag identity. The
tag workflow publishes the attestation bundle and normalized metadata; a
release that lacks those files is not valid for signed installer verification.
Run the command from a trusted m80 checkout or installed verifier distribution;
do not load the trust policy from the release dist being verified:

```sh
M80_RELEASE_COMMIT="$(git rev-list -n 1 "$M80_RELEASE_TAG")"

python3 scripts/verify-release-bundle.py \
  /tmp/m80-release-dist/m80-linux-x86_64.tar.gz \
  --release-tag "$M80_RELEASE_TAG" \
  --verify-integrity \
  --commit-sha "$M80_RELEASE_COMMIT" \
  --trust-policy docs/behaviors/release/m80-release-trust-policy.json \
  --attestation-bundle /tmp/m80-release-dist/m80-release-integrity.attestation.jsonl \
  --attestation-metadata /tmp/m80-release-dist/m80-release-attestation.json \
  --verification-time "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --rust-toolchain 1.82
```

`scripts/verify-release-bundle.py --verify-integrity` drives
`scripts/verify-release-integrity.py` after public bundle and sidecar checks so
humans and automation have one command for the full public dist verification
path.

This check is read-only and does not require root. It fails closed for wrong
tag, wrong commit, missing subject digests, unknown signers, stale keysets,
expired trust material, unsigned/downgraded material, tampered bundle or
installer bytes, failed GitHub attestation verification, unsupported schema,
and bundle metadata or asset-index tag drift.

To add a new architecture or image kind, add a new asset-index row and publish
the matching bundle, metadata sidecar, checksums, and integrity material. The
README command stays the same: the installer/bootstrapper reads the verified
index and selects the matching host tuple without changing README commands. Do
not add architecture-specific README commands unless the common installer cannot
select the tuple.

The workflow stages and token boundary for building and publishing release
artifacts are recorded in `docs/runbook/release-bundle.md`. The short version:
build jobs run with `contents: read`; the tag-only publish job is the only stage
with `contents: write`; and `scripts/lint-github-workflows.py` keeps that
boundary from drifting in CI. The publish job also runs
`scripts/release_publish_authority.py` at the mutation boundary, before the
publish decision receipt and before `gh release upload`, so the actual GitHub
context must still match the expected repository, tag ref, workflow path, job
id, token source, and write-permission posture. Run
`python3 scripts/lint-github-workflows.py` locally before release workflow
edits; it is the static m80 authority-policy and strict workflow shell check,
and it should run beside the pinned `actionlint` syntax/run-block gate:
`python3 scripts/run-actionlint.py --workflow-dir .github/workflows`.

Release and freshness workflow jobs also carry explicit job timeout budgets.
Inner command timeouts remain the primary failure detector for network fetches,
tool installation, and smoke probes; the job timeout is the final guard so a
wedged runner cannot leave release authority undecided.

| Workflow | Job | Timeout |
| --- | --- | --- |
| `.github/workflows/ci.yml` | `test` | 45 minutes |
| `.github/workflows/ci.yml` | `audit` | 20 minutes |
| `.github/workflows/release-artifacts.yml` | `build-release-artifacts` | 90 minutes |
| `.github/workflows/release-artifacts.yml` | `publish-release-artifacts` | 30 minutes |

`scripts/lint-github-workflows.py` requires every workflow whose filename
contains `release`, `latest`, `freshness`, `proof`, or `publish` to put
`timeout-minutes` on every normal job, with a maximum of 120 minutes. A reusable
workflow job that cannot own `timeout-minutes` must carry a YAML comment in the
job body such as `# m80-lint: reusable-timeout-minutes=45`; that number is the
documented budget of the called workflow and is linted against the same maximum.
Release build and publish jobs upload partial `/tmp/m80-release-*` diagnostics
on ordinary step failures where the runner still reaches the diagnostic upload
step. If the job-level timeout fires first, GitHub names the timed-out job in
the run UI; the budget table above is the source of truth for which guard fired.

The release readiness gate source of truth is
`docs/behaviors/release/release-readiness-lanes.json`. It names the required
lanes, proof kinds, allowed substrate kinds, required/warning severity,
status taxonomy, tag/commit fields, digest fields, and remediation command
fields that a future aggregate readiness receipt must consume before
publish/latest authority can move. Validate changes with
`python3 scripts/verify-release-readiness-config.py`; CI runs that validator and
its negative fixture suite beside the release script tests.

## Publish Decision Receipt

Before the tag publish job mutates GitHub release state, it writes and validates
`m80-release-publish-decision.json` with
`scripts/release_publish_receipt.py`. The receipt binds the release tag, commit
SHA, workflow run id/attempt, actor, repository, tag ref, upload manifest
digest, proof input digest, and public asset list. The mutating
`gh release upload` step runs only after the receipt validates against the
downloaded workflow artifact bytes.

The receipt is uploaded as a durable workflow artifact on success and is also
included in failed publish diagnostics when upload or post-upload verification
fails after the receipt step. It is decision evidence, not a replacement for
the signed release integrity predicate or attestation bundle. The behavior
contract lives in
`docs/behaviors/release/publish-decision-receipt.md`.

After upload, the publish job re-downloads the public release assets and writes
`m80-release-remote-assets.json`. That inventory records the GitHub release id,
remote asset ids, names, sizes, SHA256 digests, browser download URLs, and
timestamps for the bytes GitHub is serving. The digest is computed from the
re-downloaded file, not from the local dist directory. Missing metadata,
duplicate asset names, stale bytes, or incomplete redownloads fail before any
later latest-promotion gate can trust the remote state. The behavior contract
lives in `docs/behaviors/release/remote-asset-inventory.md`.

## Quickstart Proof Artifact

The release workflow writes `m80-quickstart-proof-hostless.json` into the
`m80-release-dist` `actions/upload-artifact` payload and validates it with
`scripts/verify-quickstart-proof.py` before upload. The publish job validates
the same proof again after `actions/download-artifact`. The hostless proof uses
the same schema as future `real-kvm` and freshness proofs, but its substrate
summary must say it is not a real-KVM run-smoke proof.

Inspect a downloaded proof with:

```sh
scripts/verify-quickstart-proof.py \
  m80-quickstart-proof-hostless.json \
  --artifact-root /path/to/m80-release-dist \
  --release-tag "$M80_RELEASE_TAG"
```

The schema and inspection contract live in
`docs/behaviors/release/quickstart-proof-artifacts.md`.
The normal release proof command is the same post-install smoke snippet used by
the README:

<!-- m80:quickstart-snippet post-install-smoke start -->
```sh
m80 run -- echo hello
```
<!-- m80:quickstart-snippet post-install-smoke end -->

The public echo proof is paired with an expected-nonzero process fixture so the
release smoke proves exit-code passthrough as well as stdout/stderr capture.

## Host Train Proof

Before promoting a release on a target host, save a preflight proof:

```sh
m80 preflight --json > release-preflight-proof.json
```

The proof payload contains `data.runtime_profile` plus
`data.host_prerequisites`. `data.runtime_profile` names the selected profile,
active install pointer, artifact/helper paths, and release tag.
`data.host_prerequisites` is the `HostPrerequisiteResult`; it must contain a
`Firecracker binary` check whose expected and observed version fields record
the Firecracker version, and a `Jailer binary` check whose expected and
observed version fields record the jailer version. The train policy source is
`crates/m80-preflight/src/firecracker_train.rs`; the CVE-floor table source is
`crates/m80-preflight/src/cve_floor.rs`.
