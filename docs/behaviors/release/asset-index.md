# Release Asset Index

The release asset index is the machine-readable contract that lets
quickstart/bootstrapper code select a bundle without inferring semantics from a
filename. The installer/bootstrapper source path reads the verified index
before bundle download, selects the default bundle by release tag, OS,
architecture, and image kind. The current flat-artifact quickstart path still
requires an explicit artifact URL until it moves fully onto release bundles. An
explicit bundle URL override bypasses index selection.

The published index is `m80-release-assets.json`. The release packager derives
it from the assembled tuple artifacts: the default package artifact plus any
`--extra-tuple-manifest` inputs. Its checksum sidecar is
`m80-release-assets.json.sha256`, and the public `SHA256SUMS` covers the index
alongside every indexed bundle, bundle checksum sidecar, metadata sidecar,
metadata checksum sidecar, `install.sh`, `m80-bootstrap-selector.tsv`, and
`m80-release-build.json`.
Installer/bootstrapper code follows this order: fetch the pinned index and its
sidecar for the same concrete release tag, verify the index sha256, and only
then parse JSON or select a host tuple. `file://` fixture indexes use the same
checksum-sidecar verifier as remote release indexes.

The index uses `schema_version: 1` and has one top-level `release_tag`. Every
bundle row records:

- `name` and `url`;
- `sha256` and `size_bytes` for the bundle tarball;
- `metadata_name` and `metadata_sha256` for the byte-identical bundle metadata
  sidecar;
- `checksum_name`, `signature_name`, and `attestation_name` proof references;
- `target`, `os`, and `arch`;
- `image_kind`;
- `release_tag` and `m80_version`;
- `guest_protocol_version`;
- `manifest_schema_version`;
- `expected_firecracker_version`.

The bundle `sha256` and `size_bytes` fields are installer inputs, not advisory
metadata. Indexed release-tag and bootstrap installs pass them through to the
release-material verifier with the selected bundle URL and checksum asset name.
Verification refuses to extract if `size_bytes` is missing, zero, or different
from the downloaded bundle length, or if the indexed sha256 disagrees with the
checksum sidecar or computed bundle digest. Explicit local `file://` fixture
installs bypass index selection and are the only path that may omit these
indexed size and digest fields.

The default Linux quickstart row is the single asset with `os: "linux"`,
`arch: "x86_64"`, and `image_kind: "minimal"` whose `release_tag` and
`m80_version` match the running release binary. Duplicate defaults fail closed;
duplicate default rows are invalid for selection.
Missing defaults fail closed. Rows for the wrong architecture, wrong image
kind, wrong tag, or wrong m80 version fail with typed diagnostics that name the
requested tuple and available alternatives.
Diagnostics name the requested `os`, `arch`, `image_kind`, release tag, and
binary version when those fields are relevant. They also name available tuples
as `<os>/<arch>/<image_kind>@<m80_version>` and include either `--bundle-url`
for an explicit compatible bundle or a pinned release `install.sh` URL when a
matching tagged release is visible in the index.

Release dev builds do not select release assets from the index. A dev build
needs an explicit local bundle URL or artifact override so it cannot silently
consume the mutable public latest release. Explicit bundle URLs bypass index
selection but do not bypass later bundle metadata and integrity verification.

The parser rejects unknown fields, unsupported schemas, empty required fields,
non-SHA-256 digest strings, zero size/schema/protocol numbers, and rows whose
`target` does not match `os` plus `arch`. This keeps the index strict enough
for future OS, architecture, or image-kind expansion without changing the
README quickstart command.

Remote index fetches use explicit 10-second curl connect and 120-second
total-time bounds for both `m80-release-assets.json` and
`m80-release-assets.json.sha256`. Slow fixtures, offline endpoints, HTTP
failures, and unsupported redirects fail as bounded asset-index selection
errors. They fail before bundle download, extraction, tuple selection, or
install-root writes. These fetch bounds only prove the index selection path
fails promptly; they do not promise later bundle download success.

Fetch diagnostics name the index URL or local path, checksum sidecar URL or
path, expected sha256, observed sha256, release tag, requested OS/arch, image
kind, and whether the failure happened before or after checksum verification.
Missing index bytes, missing sidecars, checksum mismatch, invalid JSON after a
valid checksum, stale schema, and index `release_tag` drift all fail before
bundle download, extraction, tuple selection, or active install state writes.

Installer JSON failures for asset-index selection are emitted as the final
stderr JSON envelope with stdout empty. The envelope `data` object uses
`variant: "ReleaseAssetIndex"`, `exit_code: 6`, and stable diagnostic fields:
`code`, `detail`, `requested_os`, `requested_arch`,
`requested_image_kind`, `requested_release_tag`,
`requested_m80_version`, optional `index_url` and
`fetch_url` plus `checksum_verification` for fetch-path errors,
`available_tuples`,
`available_image_kinds`, `available_m80_versions`, plus `repair_url` and
`repair_command` when a concrete repair is known. Human stderr prints the same
fields as `asset_index_code=...`, `requested_*`, `index_url=...`,
`fetch_url=...`, `checksum_verification=...`, `available_*`, and `repair_*`
lines after the error summary.

The stable `code` values are:

- `unsupported_host_tuple` for unsupported OS/architecture rows;
- `missing_image_kind` for a missing `minimal` image kind on an otherwise
  supported tuple;
- `stale_asset_index` when the verified index has the right tuple/tag but no
  row for the running m80 version;
- `binary_tag_mismatch` when the install source and running binary identity
  disagree before bundle selection;
- `duplicate_default_bundle` for multiple matching default rows;
- `dev_build_refused` when an unreleased binary tries to select public release
  assets implicitly;
- `index_tag_mismatch`, `asset_release_tag_mismatch`, and
  `target_tuple_mismatch` for stale or internally inconsistent index rows;
- `invalid_json`, `unsupported_schema`, `missing_field`, and `invalid_field`
  for strict schema failures;
- `unsupported_url`, `local_read_failed`, `download_spawn_failed`,
  `download_failed`, `redirect_unsupported`, `checksum_invalid`,
  `checksum_mismatch`, and `verified_index_invalid` for fetch or integrity
  failures before tuple selection.

The release also publishes `m80-bootstrap-selector.tsv`, a non-executable
line-oriented projection of the canonical JSON index for the no-installed-binary
POSIX installer path. It contains:

- `schema_version`;
- `release_tag`;
- a fixed `columns` row;
- one `row` per index asset with `os`, `arch`, `image_kind`, bundle name/URL,
  bundle sha256, `size_bytes`, metadata name/sha256, checksum name, proof asset
  names, and `m80_version`.

The selector is generated mechanically from `m80-release-assets.json`, is
checksum-covered, appears in `SHA256SUMS`, and is represented in release
integrity material. Each row's bundle, metadata sidecar, bundle checksum
sidecar, metadata checksum sidecar, and any named detached signature are also
represented in both the public checksum manifest and release integrity
subjects. Selector values are conservative ASCII shell tokens:
whitespace, control characters, and shell metacharacters are invalid; nullable
proof fields use `-`. `signature_name` is nullable in schema v1 because there
is no detached-signature lane, but `attestation_name` is required for official
signed release rows and names `m80-release-integrity.attestation.jsonl`. It is
not a second source of truth: verification compares selector rows back to the
JSON index and rejects stale tag, missing tuple, duplicate tuple, stale
digest/size, unsupported schema, shell-unsafe tokens, or hand-edited drift.

Release publication must generate the index from the actual assembled dist
files, never by editing JSON or TSV after packaging. To add a tuple, package the
tuple bundle and metadata sidecar, pass them to
`scripts/package-release-bundle.py --extra-tuple-manifest`, upload the generated
dist as a unit, then re-download the public release and validate the index
against the uploaded tarballs, metadata sidecars, checksum sidecars, build
manifest, and `SHA256SUMS` with
`python3 scripts/verify-release-integrity.py ... --dist-dir <release-dist>`.
A new architecture or image kind is a new row in the index, not a new README
quickstart command.

The current publisher is checksum-covered and attestation-referenced:
`signature_name` is null, and `attestation_name` is
`m80-release-integrity.attestation.jsonl`. Checksum coverage proves the exact
bytes that the selector parsed; it is not a substitute for signed release integrity
verification. Signed-release verification fails closed when an
asset-index row omits `signature_name` or `attestation_name`, leaves
`attestation_name` empty, names a stale attestation bundle, names a signature
file that is absent from the release dist, or drifts from the release tag,
bundle metadata version, bundle tarball, metadata sidecar, checksum sidecar, or
selector row.

The POSIX bootstrapper still performs checksum-only selection first so it can
find the tuple-specific bundle URL without an installed `m80` binary. That
selection is only an input to signed parity verification. Before bundle
download, `install.sh` verifies release integrity material that covers both
`m80-release-assets.json` and `m80-bootstrap-selector.tsv`, then compares the
selected selector tuple back to the canonical index row. If either file is
hand-edited and its plain `.sha256` sidecar is refreshed, the install still
fails before bundle download with release tag, OS, arch, image kind, selector
URL, index URL, and the mismatched tuple field in the diagnostic.
