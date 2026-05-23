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
workspace package version `0.2.11`, the expected release tag is `v0.2.11`.
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

<!-- m80:freshness-status start -->
Public installer status: public proof green for `v0.2.11`. Latest and pinned install URLs were verified from unauthenticated public release assets. Proof: [latest-and-pinned-url-proof](../behaviors/release/release-readiness-public-access.json).
<!-- m80:freshness-status end -->

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
for asset in install.sh install.sh.sha256 SHA256SUMS m80-release-integrity.json m80-release-integrity.attestation.jsonl m80-release-attestation.json; do
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
Use latest for the fastest interactive Linux install, pinned for automation or
reproducible bug reports, verified handoff when a trusted checkout must inspect
`install.sh` before sudo, `m80 install-status` for first repair triage,
`m80 update --check` for read-only freshness, and rollback cleanup only after
choosing a previously verified active version.
Legacy artifact-only `releases/latest` tarball and raw `main` installer flows
are documented only in the
[`legacy quickstart migration note`](../behaviors/release/legacy-quickstart-hard-cutover.md).

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

The public `install.sh` performs a local preflight before release downloads.
Its minimal shell dependency set is `curl`, `python3`, `sha256sum`, `tar`,
`mktemp`, `chmod`, `mkdir`, `rm`, `uname`, `wc`, and `id`. A real install into
the default `/opt/m80` root must run as root; use the documented `curl | sudo
sh` form. `--dry-run` and explicit non-`/opt` `--install-root` fixture paths do
not require root at the script preflight.

Latest metadata resolution is bounded separately from release-asset downloads:
both the initial and guard GitHub metadata fetches use a 10 second connect
timeout, 120 second total timeout, two retries, and a one second retry delay
before emitting install handoff JSON. URL-mode failures name the fetch role,
metadata URL, and curl failure class. Fixture mode stays network-free and does
not require curl.

Scheduled hostless freshness uses the same bounds through
`scripts/release_freshness.py`. The verifier checks GitHub latest metadata, the
documented latest install URL, the pinned install URL for the resolved stable
tag, every installer-consumed public asset URL, and concrete docs-linked
release install URLs. Failures name the URL, release tag, asset, freshness role,
curl exit code, and source such as `docs:README.md:<line>`, so stale docs or
missing public assets are repairable without rerunning a privileged smoke.
`.github/workflows/latest-freshness.yml` runs this verifier on a scheduled and
manual read-only hostless lane. Its `latest-freshness-public` concurrency group
uses `cancel-in-progress: false` so an overlapping run does not discard the
older run's evidence. It uploads proof, drift, stdout, and stderr workflow
artifacts with `if: always()`, then a separate publish job uploads the successful
`m80-latest-freshness-proof.json` to the resolved public release with
`--clobber`.

The uploaded `m80-latest-freshness-proof.json` is the reusable input for
freshness status repair and release-readiness gates. It records the proof schema
version, generated time, workflow run id, resolved latest tag, public command
inventory digest, tag agreement, integrity result, hostless fixture-install
result, substrate/auth details, and failure taxonomy. The verifier validates
that proof before upload, including failure proofs, so a stale README command,
missing public asset, tag mismatch, integrity mismatch, or malformed proof can
file a repair bead without rerunning the public latest check.

Closing public-latest freshness leaves is a two-step process. Hostless
fake-release tests may land as scaffold evidence, but a closed
`requires-verified-close` leaf whose text asks for public latest, public release,
or public asset proof must cite a committed proof from the public unauthenticated
run (`network_target=public-github-release`, `auth_state=unauthenticated-public-read`,
`public_owner=moradology`, `public_repo=m80`, `fixture_source=false`) or an
explicit real-KVM proof where the leaf asks for real substrate. The tracker
policy rejects local/fake/fixture substrates for those public-proof leaves.

The remote stable status artifact consumed by `m80 update --check` is
`m80-latest-freshness-proof.json` from the public latest GitHub release asset
set. The publish lane produces the docs copy from
`release-readiness-public-access.json` with
`scripts/update-freshness-status-from-public-access.py`, optionally passing
`--safety-floor <path>` when a yanked/minimum-safe policy is active, then
validates it with `scripts/verify-freshness-status.py`. The scheduled
`.github/workflows/latest-freshness.yml` lane independently re-reads the public
latest release, validates the same latest/pinned URLs and installer-consumed
assets, and uploads its proof/drift artifacts for repair. `m80 update --check`
fetches only that status asset by default, or the operator-provided
`--latest-status-url`; it never downloads bundles or mutates the install root.
Current public proof:
`docs/behaviors/release/public-latest-update-check-v0.2.11.json`.

The freshness status contract is defined in
`docs/behaviors/release/freshness-status.md` and validated by
`scripts/verify-freshness-status.py`. A green status is only `public_green` when
it references unauthenticated public proof for both latest and pinned install
URLs; fixture-only proof remains scaffolded and must not be rendered as a
public success. Safety-floor data is advisory unless a separate release policy
or CI gate names this status artifact as release-blocking input. The
`safety_floor` object has no blocking boolean; do not make release blocking
depend on the mere presence of `minimum_safe_tag` or `yanked_releases`.
Safety-floor contradictions always block green publication before the checked
status file is replaced.

The current in-repo release dispositions live in
[`freshness-failure-policy.md`](../behaviors/release/freshness-failure-policy.md).
If a safety-floor condition is meant to block release/latest promotion as a
policy matter, add or update that named policy/gate instead of relying on
implicit status schema semantics.

The release operator or security maintainer owns the JSON object passed as
`--safety-floor <path>`. The publisher embeds that object unchanged except for
normal JSON rendering, then validates it against the resolved latest tag before
replacing `freshness-status-docs.json`. A failed safety-floor publication means
the previous public status remains the last trusted status; do not render the
candidate as green and do not hand-edit README or runbook markers around the
failure. If the policy is meant to block a release, update the explicit policy
or CI gate in the same change and name the artifact it consumes.

If safety-floor validation fails, do not publish or render the candidate green
status. Repair the policy source first:

- Lower `minimum_safe_tag.tag` to a stable release no newer than the resolved
  latest tag, or publish and prove the newer release first.
- Remove duplicate `yanked_releases` rows.
- If the resolved latest tag is yanked, provide a pinned replacement command
  for a safe release.
- Point every replacement command at a non-yanked release at or above
  `minimum_safe_tag`.
- Move `safety_floor.published_at` no later than the status `generated_at`, and
  move every yanked row's `published_at` no later than `safety_floor.published_at`.
- Keep at least one evidence pointer, `advisory_url` or `issue_id`, on every
  policy row.

Then regenerate and verify before rendering:

```sh
scripts/update-freshness-status-from-public-access.py \
  ${SAFETY_FLOOR_PATH:+--safety-floor "$SAFETY_FLOOR_PATH"}
python3 scripts/verify-freshness-status.py \
  docs/behaviors/release/freshness-status-docs.json \
  --artifact-root docs/behaviors/release \
  --docs-root .
python3 scripts/render-freshness-status.py
python3 scripts/render-freshness-status.py --check
```

Freshness drift repair starts with the uploaded
`m80-latest-freshness-drift.json` artifact. Its schema is documented in
`docs/behaviors/release/freshness-drift-evidence.md` and validated with
`python3 scripts/freshness_drift_evidence.py --validate <artifact>`. Use it
before rerunning the scheduled check; it carries the failure class, source
URL/docs snippet, tag context, expected/observed values, workflow run id, and a
safe stderr excerpt for the repair bead.

The verifier also checks that the GitHub latest release metadata contains every
installer-consumed public asset with the expected name, URL, size when reported,
and `sha256:` digest. It then reads `SHA256SUMS` and every published
per-asset `.sha256` sidecar as content, compares those sums to the same
metadata digests, and records the proving checksum sources in the freshness
proof. With an asset-index fixture, the bundle and metadata digests must agree
with the index row before URL liveness checks run.
Freshness failure handling is configured in
`docs/behaviors/release/freshness-failure-policy.json` and validated with
`python3 scripts/verify-freshness-failure-policy.py`. The taxonomy maps
`network-transient`, `stale-latest`, `missing-public-asset`, `docs-drift`,
`checksum-mismatch`, `provenance-mismatch`, `public-release-unavailable`,
`real-kvm-substrate-unavailable`, and `verifier-schema-drift` to a primary
operator action: retry-only, open/update one repair bead, block the next
release/latest promotion, page a maintainer, or require manual operator
confirmation. The verifier appends the class's cataloged `repair_command` to
failure diagnostics. Add a new class by teaching the verifier to emit it and
adding a single JSON policy row in the same diff; CI rejects verifier-emitted classes
that are absent from the policy.

Current catalog commands:

| Failure class | First command |
| --- | --- |
| `network-transient` | `python3 scripts/release_freshness.py --docs-root . --json` |
| `stale-latest` | `br show m80-o3uh9.21.8` |
| `missing-public-asset` | `br show m80-o3uh9.21.7` |
| `docs-drift` | `python3 scripts/render-freshness-status.py --check` |
| `checksum-mismatch` | `python3 scripts/verify-release-integrity.py --help` |
| `provenance-mismatch` | `python3 scripts/verify-release-integrity.py --help` |
| `public-release-unavailable` | `br show m80-o3uh9.21.8` |
| `real-kvm-substrate-unavailable` | `br show m80-o3uh9.18` |
| `verifier-schema-drift` | `python3 scripts/verify-freshness-failure-policy.py` |

The shell installer bounds every release-asset download before the bundled
`m80 install` binary can take over. Each fetch uses a 10 second connect timeout,
120 second total timeout, two retries, and a one second retry delay. Failure
diagnostics name the release tag, asset, URL, curl failure class, and whether
release verification had started. Offline or private-network hosts are expected
to fail clearly; this policy is not an offline install guarantee.

First-run support reports should cite the stable
[`quickstart troubleshooting matrix`](../behaviors/release/quickstart-troubleshooting-matrix.md#network)
ID before prose diagnosis. The matrix covers
[`missing-local-tool`](../behaviors/release/quickstart-troubleshooting-matrix.md#missing-local-tool),
[`host-prerequisite`](../behaviors/release/quickstart-troubleshooting-matrix.md#host-prerequisite),
[`stale-profile`](../behaviors/release/quickstart-troubleshooting-matrix.md#stale-profile),
and
[`process-smoke-failed`](../behaviors/release/quickstart-troubleshooting-matrix.md#process-smoke-failed)
without duplicating repair advice in this runbook.

`scripts/verify-install-handoff.py` verifies downloaded `install.sh` bytes and
their signed release-integrity subject before automation runs local verified
bytes with `sudo`. It prints the release tag, source commit,
`install_sh_sha256`, and verified asset names before the privilege-sensitive
handoff. See
`docs/behaviors/release/verified-install-handoff.md`.

## Existing Config And Profile Cutover

`m80 install` owns the generated default selector files for the installed
product: `/etc/m80/config.toml` plus `/etc/m80/profiles/default.toml` for the
default root, or `<install-root>/config.toml` plus
`<install-root>/profiles/default.toml` for install-root fixtures. The installer
writes those files only when they do not exist, or when their bytes already
match the generated installed selector.

If either file exists with operator-owned contents, install fails before active
state is changed. The diagnostic names the existing path, the proposed path, and
two exact choices: rerun the same install command with
`--adopt-existing-config`, or back up the file with the printed `cp -a` command
and rerun. Adoption is a hard cutover; m80 does not merge operator keys into the
generated installed selector. If a later finalization step fails after adoption,
the previous config/profile bytes are restored before the command exits.
The behavior contract is recorded in
`docs/behaviors/release/install-config-preservation.md`.

## Rollback And Install Cleanup

Rollback is pointer-first. Switch the active pointer to a previously verified
version, confirm status, then clean the now-inactive directory:

<!-- m80:quickstart-snippet rollback-cleanup start -->
```sh
sudo ln -sfnT -- /opt/m80/versions/<previous-tag> /opt/m80/active
m80 install-status
sudo m80 install-cleanup --release-tag <old-tag>
```
<!-- m80:quickstart-snippet rollback-cleanup end -->

For install-root fixtures, pass the same root to status and cleanup:

```sh
m80 install-status --install-root <install-root>
m80 install-cleanup --install-root <install-root> --release-tag <old-tag>
```

`m80 install-cleanup` removes only one direct child of
`<install-root>/versions/` and refuses malformed directories or the active
version by default. `--remove-active` is the explicit break-glass path; it
unlinks the active pointer before removal and leaves no active installed release
selected. The behavior contract is recorded in
`docs/behaviors/release/install-cleanup.md`.

## Installed Status Evidence

Release evidence captures include the installed status JSON from the candidate
install root:

```sh
m80 --json install-status > install-status.json
```

Capture these fields from `install-status.json` when recording release
evidence:

- `schema_version`: must be `1`.
- `status`: must be `healthy_active_release` for a default installed release
  evidence capture.
- `install_root`: the root inspected by the command, usually `/opt/m80`.
- `active.release_tag` and `active.install_dir`: the selected release tag and
  version directory.
- `selected_config.default_profile`,
  `selected_config.default_profile_source`, and
  `selected_config.explicit_override`: evidence that the installed system
  default profile, not an override, selected the runtime profile.
- `selected_profile.name`, `selected_profile.body_source`,
  `selected_profile.install_dir`, and `selected_profile.release_tag`: evidence
  that `m80 run` will use the same installed version as the active pointer.
- `metadata.bundle_metadata.path`,
  `metadata.host_binaries_manifest.path`,
  `metadata.install_provenance.path`, and
  `metadata.proof_cache_manifest.path`: paths to the installed metadata and
  preserved public proof material.
- Each metadata status and sha256 field: proof that the files were present and
  matched the status reader's local digest checks.
- `proof_cache.status`: must be `available`.
- `proof_cache.manifest_digest`, `proof_cache.cache_dir`,
  `proof_cache.manifest_path`, `proof_cache.materials[]`,
  `proof_cache.trust_policy.path`, `proof_cache.trust_policy.sha256`, and
  `proof_cache.verifier_versions.*`: offline evidence of the public material
  and trust policy verified at install time.
- `proof_cache.diagnostics` and `proof_cache.repair_command`: must be empty
  for green evidence; non-empty values mean local saved trust material must be
  repaired or reinstalled before reuse.
- `diagnostics` and `mismatches`: must be empty for a default installed release
  evidence capture. Non-empty values are repair inputs, not green evidence.
- `next_action.kind`: must be `ready`; `next_action.command` should be
  `m80 run -- echo hello`.

This status capture proves the local installed selector/profile/metadata shape.
It does not replace the real-KVM smoke, public release proof, host-prerequisite
preflight, or freshness lanes.

For local install repair triage, start with:

<!-- m80:quickstart-snippet repair-status start -->
```sh
m80 install-status
```
<!-- m80:quickstart-snippet repair-status end -->

For operator update status, use the read-only check:

<!-- m80:quickstart-snippet freshness-check start -->
```sh
m80 update --check
```
<!-- m80:quickstart-snippet freshness-check end -->

The check reads the same local active install and proof-cache state, then
compares it with bounded latest freshness metadata. It reports `current`,
`outdated`, `unknown_offline`, `stale_latest_metadata`, `prerelease_active`,
`ineligible_active`, `local_dev_install`, `install_unhealthy`, `yanked`, and
`unsafe`, and prints an exact pinned install command when a newer safe release
is known. It reports `active_kind` separately from freshness state so prerelease,
local-dev, missing active, and stale active metadata cases are visible without
guessing. It does not write the install root, flip the active pointer, refresh
proof cache material, or download a bundle. See
`docs/behaviors/release/update-check.md`.

When a safe update is known, the copy-ready repair line is the same command the
renderer emits as `next_command`:

```text
next_command=curl -fsSL https://github.com/moradology/m80/releases/download/v1.2.4/install.sh | sudo sh
```

## Upgrade And Rollback Evidence

Release evidence for an upgrade path must prove the target became active only
after verification and that the previous release stayed available for explicit
rollback. Capture or reference artifacts showing:

- `install-status.json` has `status: healthy_active_release`,
  `active.release_tag` equal to the newer target, empty `diagnostics`, empty
  `mismatches`, `proof_cache.status: available`, and `next_action.kind: ready`.
- The installer transcript or machine summary names the newer
  `active_version_dir`, `previous_active_release_tag`, and
  `previous_active_version_dir`.
- The versioned directory for `previous_active_release_tag` still exists under
  `<install-root>/versions/` after the active flip.
- `last-install-attempt.json` either records `successful_upgrade` for the
  target tag or is absent only in a dry-run/proof path that did not mutate the
  install root.

Rollback evidence must prove intent and avoid treating downgrade-by-install as
rollback. Capture or reference artifacts showing:

- The rollback command was the active-pointer command, not an older pinned
  installer command:
  `sudo ln -sfnT -- '<install-root>/versions/<previous-tag>' '<install-root>/active'`.
- Post-rollback `install-status.json` names `<previous-tag>` as
  `active.release_tag`, has `status: healthy_active_release`, and has empty
  `diagnostics` and `mismatches`.
- Any cleanup after rollback used `m80 install-cleanup --release-tag <old-tag>`
  for an inactive direct child of `<install-root>/versions/`.
- A refused older install, when exercised, records `downgrade_refused` in
  command output or `last-install-attempt.json` and leaves the pre-attempt
  active release selected.

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

Prerequisites for the signed v1 verifier: `python3`. The verifier is native to
the installed m80 CLI and the checked-in Python scripts; it does not require
GitHub CLI on the target host before reading release proof material, downloading
official release assets, or touching active install state.

Verify a downloaded dist directory before treating a release as signed. This
one read-only command verifies the bundle tar contract, bundle checksum,
installer checksum, metadata sidecar, public sidecars, signed predicate,
attestation metadata, native attestation bundle, and tag identity. The
tag workflow publishes the attestation bundle and normalized metadata; a
release that lacks those files is not valid for signed installer verification.
Direct official bundle URLs use the same trust path; the current public proof is
`docs/proofs/release/m80-o3uh9.15.11.8-direct-official-url-v0.2.11.json`.
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

The official direct-URL installer path uses the same GitHub Artifact
Attestation identity before downloading the selected bundle tarball. The native
verifier checks the Sigstore bundle shape, DSSE in-toto payload, predicate
subject digest, repository, workflow path, tag ref, source commit, and
github-hosted runner provenance from the downloaded attestation bundle.

This check is read-only and does not require root. It fails closed for wrong
tag, wrong commit, missing subject digests, unknown signers, stale keysets,
expired trust material, unsigned/downgraded material, tampered bundle or
installer bytes, failed native attestation bundle verification, unsupported schema,
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
id, token source, and write-permission posture. That command writes
`m80-release-token-authority.json`; a failed authority check preserves the
failed receipt with the failing context and probe result so the operator can fix
workflow permissions, release metadata access, repository settings, or the
wrong run context before any release mutation. Run
`python3 scripts/lint-github-workflows.py` locally before release workflow
edits; it is the static m80 authority-policy and strict workflow shell check,
and it should run beside the pinned `actionlint` syntax/run-block gate:
`python3 scripts/run-actionlint.py --workflow-dir .github/workflows`.

The token authority receipt is uploaded as
`m80-release-token-authority-<run id>` on success and is also present inside
failed publish diagnostics under the publish upload directory. It records stable
diagnostics only: actor, repository/ref, workflow path/job, run id/attempt,
token source name, policy id/digest, and probe names/results. It must not carry
token bytes, authorization headers, raw environment maps, or secrets. Repair by
the field that failed: context fields mean rerun from the tag publish workflow;
token-source or missing-token failures mean fix `GH_TOKEN` /
`M80_RELEASE_TOKEN_SOURCE` wiring; failed `release_metadata` probes mean fix
repository release access, tag state, or GitHub API availability; and
`policy_digest` drift means update the policy, tests, and runbook together
before relying on the new authority shape.

The tag publish job targets the protected GitHub Actions environment
`m80-release-publish`. Build, PR dry-run, hostless verification, freshness, and
real-KVM smoke jobs do not request that environment. A normal release uses
`github.token` with job-scoped `contents: write`; it does not require a personal
access token. The environment should require maintainer approval before the
publish job starts. At that point the build job has produced the release
artifact handoff; after approval, the publish job still validates the upload
manifest, hostless quickstart proof, proof ledger, integrity material,
repository protection audit, token authority receipt, and publish decision
receipt before any release mutation. `scripts/lint-github-workflows.py` fails
if `publish-release-artifacts` loses the environment, if another
release-artifacts job requests it, if release mutation commands move outside the
publish job, or if the publish job can run without the validated build handoff
dependency.

Repository settings are audited before release mutation because they can drift
outside git. The required settings are:

- `main` branch protection has required status checks.
- A repository ruleset targets release tags matching `v*`.
- The `m80-release-publish` environment has required reviewers.

Run the same audit locally with a token that can read repository settings:

```sh
GH_TOKEN=<token> scripts/repository_protection_audit.py \
  --repository moradology/m80 \
  --branch main \
  --tag-pattern "v*" \
  --environment m80-release-publish \
  --out /tmp/m80-repository-protection-audit.json \
  --write
```

The tag publish workflow runs that audit before the token authority receipt and
uploads `m80-repository-protection-audit-<run id>` on success. If the audit is
unavailable or reports weaker settings than this runbook requires, the publish
job fails before upload/latest authority. Fix the named setting in the audit's
`remediation` field, then rerun the tag workflow.

If the publish job is waiting on `m80-release-publish`, inspect the run summary
and approve or reject it there; do not rerun the build just to satisfy the gate.
If the job created or uploaded a draft but did not reach latest promotion, use
the uploaded `m80-release-publication-plan-*`,
`m80-release-publish-decision-*`, `m80-release-token-authority-*`, and failed
diagnostic artifacts to decide whether to rerun the same tag workflow or delete
only the draft release:

```sh
gh release delete <version> --yes
```

Release and freshness workflow jobs also carry explicit job timeout budgets.
Inner command timeouts remain the primary failure detector for network fetches,
tool installation, and smoke probes; the job timeout is the final guard so a
wedged runner cannot leave release authority undecided.

Before closing release beads locally, run the same whitespace gate CI runs:

```sh
git diff --check
```

On GitHub, `.github/workflows/ci.yml` runs a changed-line equivalent on both
push and pull request events before host tool installation and Rust build/test
steps. Pull requests diff against `github.event.pull_request.base.sha`; pushes
diff against `github.event.before`, with branch-creation pushes falling back to
the repository root commit.

The durable publish-proof schema entrypoint is `m80-release-evidence.json`,
validated by `scripts/release_evidence_bundle.py`. Schema version 2 records
release tag, commit, workflow run id, m80 version, resolved install tag,
required and missing readiness lane ids, and digests for the upload manifest,
build handoff, publish decision receipt, proof ledger, public assets, and
workflow-only artifacts without embedding host paths or token material. The
top-level proof ledger ref points at `m80-release-proof-ledger.jsonl`; the
hostless proof row points at `m80-quickstart-proof-hostless.json`. Release
workflow emission and upload are handled by the evidence collector/verifier
leaves in the `m80-o3uh9.13.39` family.

| Workflow | Job | Timeout |
| --- | --- | --- |
| `.github/workflows/ci.yml` | `audit` | 20 minutes |
| `.github/workflows/ci.yml` | `test` | 45 minutes |
| `.github/workflows/latest-freshness.yml` | `hostless-public-freshness` | 10 minutes |
| `.github/workflows/latest-freshness.yml` | `publish-latest-freshness` | 5 minutes |
| `.github/workflows/release-artifacts.yml` | `build-release-artifacts` | 90 minutes |
| `.github/workflows/release-artifacts.yml` | `publish-release-artifacts` | 30 minutes |

`scripts/lint-github-workflows.py` requires every workflow listed as
`release-authority`, `latest-freshness`, or `proof` in
`docs/behaviors/ci/workflow-policy-scope.json` to put `timeout-minutes` on every
normal job, with the exact value from
`docs/behaviors/ci/workflow-timeout-budgets.json` and a maximum of 120 minutes.
A reusable workflow job that cannot own `timeout-minutes` must carry a YAML
comment in the job body such as `# m80-lint: reusable-timeout-minutes=45`; that
number is the documented budget of the called workflow and is checked against
the same config.

When adding, renaming, or splitting a release, proof, publish, latest, or
freshness workflow, update `docs/behaviors/ci/workflow-policy-scope.json` in the
same diff before merging. Use `release-authority` for tag release build/publish
authority, `latest-freshness` for the scheduled public latest freshness lane,
`proof` for proof-producing release workflows, and `ordinary-ci` for normal CI.
The linter rejects missing entries, duplicate entries, unknown scopes, missing
configured files, and guarded-looking filenames that are not represented in the
policy.
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
digest, `m80-release-token-authority.json` digest,
`m80-release-proof-ledger.jsonl` digest, separate
`m80-quickstart-proof-*.json` proof refs, and public asset list. The mutating
`gh release upload`, draft-publication, and latest-promotion steps run only
after the receipt validates against the downloaded workflow artifact bytes and
the token authority receipt. The ledger is the durable JSONL index; the
quickstart proof JSON is the per-lane proof payload. They are never
interchangeable receipt fields.

The publish decision receipt is uploaded as a durable workflow artifact on
success and is also included in failed publish diagnostics when upload or
post-upload verification fails after the receipt step. It is decision evidence,
not a replacement for the signed release integrity predicate or attestation
bundle. The behavior contract lives in
`docs/behaviors/release/publish-decision-receipt.md`.

After the receipt, the publish job writes
`m80-release-publication-plan.json` with
`scripts/release_publication_plan.py`. If the tag has no GitHub release, the
job creates a draft release with `--verify-tag`, uploads the manifest-selected
public assets without `--clobber`, re-downloads and validates the uploaded
draft assets, publishes the validated draft as a public non-latest release,
re-downloads and validates the public bytes, uploads the publish receipts, and
marks it latest only after those readiness gates pass. If the public release
already exists with the complete asset name and size set, the job skips upload
and validates the served bytes. Draft, prerelease, incomplete, or
size-mismatched existing releases fail before upload with a manual recovery
instruction; for a leftover draft or failed public attempt, delete only the
release:

```sh
gh release delete <version> --yes
```

The behavior contract lives in
`docs/behaviors/release/publication-plan.md`.

After upload or validate-only rerun selection, the publish job re-downloads the
public release assets and writes `m80-release-remote-assets.json`. That
inventory records the GitHub release id, remote asset ids, names, sizes, SHA256
digests, browser download URLs, and timestamps for the bytes GitHub is serving.
The digest is computed from the re-downloaded file, not from the local dist
directory. Missing metadata, duplicate asset names, stale bytes, or incomplete
redownloads fail before any later latest-promotion gate can trust the remote
state. The same step runs rerun preflight mode against the upload manifest,
`m80-release-build.json`, and `m80-release-publish-decision.json`, so an
existing public release is accepted only when the remote asset names, ids,
sizes, digests, release tag, source commit, and receipt digests still match the
current build handoff and publish decision. The behavior contract lives in
`docs/behaviors/release/remote-asset-inventory.md`.

Before latest promotion, the publish job captures the current public release
list and writes `m80-latest-promotion-decision.json` with
`scripts/release_latest_promotion.py`. The normal rule is monotonic: the target
tag must be greater than or equal to the highest public non-draft,
non-prerelease stable tag matching `vMAJOR.MINOR.PATCH`. Drafts, prereleases,
and prerelease suffix tags do not count as the highest stable release. This is
the final release pointer move, so the decision also consumes
`m80-release-remote-assets.json` and refuses latest promotion unless the remote
asset inventory still matches `m80-release-upload-manifest.json` by release tag,
asset names, ids, sizes, and SHA256 digests. Latest depends on the public bytes
GitHub is serving, not just the local workflow artifact bytes.

Emergency rollback is explicit and auditable. Commit
`docs/operations/release-latest-rollback-receipt.json` before the protected tag
workflow runs. It must contain `kind: m80_release_latest_rollback_receipt`,
`decision: approved`, the older `release_tag`, the currently highest stable
public tag, a reason, actor, and the exact publish decision and proof ledger
digests. Stale, missing, or mismatched rollback receipts fail before
`gh release edit --latest`; normal releases do not need this file. The behavior
contract lives in
`docs/behaviors/release/latest-promotion-monotonicity.md`.

After latest promotion, the publish job also writes
`release-readiness-public-access.json` with
`scripts/release_public_access_receipt.py` from an empty `GH_CONFIG_DIR` and
with `GH_TOKEN`/`GITHUB_TOKEN` unset. This is the reusable no-auth proof that
`/releases/latest/download/install.sh` and the pinned installer URL resolve to
the same stable tag, that every public installer asset is reachable from the
public GitHub release, and that the downloaded bytes match the release
integrity/build metadata. It is required after latest promotion because the
receipt verifies `/releases/latest/download/install.sh`; fixture receipts cannot satisfy this real
public-access lane. The behavior contract lives in
`docs/behaviors/release/public-access-receipt.md`.

When that public receipt is copied into `docs/behaviors/release/`, refresh the
checked README/runbook status with:

```sh
scripts/update-freshness-status-from-public-access.py \
  ${SAFETY_FLOOR_PATH:+--safety-floor "$SAFETY_FLOOR_PATH"}
python3 scripts/render-freshness-status.py
python3 scripts/render-freshness-status.py --check
```

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
