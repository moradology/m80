#!/bin/sh
# Versioned release installer template. package-release-bundle.py renders the
# placeholder below for the concrete GitHub release.

set -eu

M80_RELEASE_TAG='@M80_RELEASE_TAG@'
M80_PUBLIC_RELEASE_OWNER='@M80_PUBLIC_RELEASE_OWNER@'
M80_PUBLIC_RELEASE_REPO='@M80_PUBLIC_RELEASE_REPO@'
M80_RELEASE_BASE_URL="https://github.com/${M80_PUBLIC_RELEASE_OWNER}/${M80_PUBLIC_RELEASE_REPO}/releases/download/${M80_RELEASE_TAG}"
M80_ASSET_INDEX_NAME='m80-release-assets.json'
M80_BOOTSTRAP_SELECTOR_NAME='m80-bootstrap-selector.tsv'
M80_INSTALL_NAME='install.sh'
M80_PUBLIC_SHA256SUMS_NAME='SHA256SUMS'
M80_RELEASE_BUILD_NAME='m80-release-build.json'
M80_RELEASE_INTEGRITY_NAME='m80-release-integrity.json'
M80_RELEASE_ATTESTATION_BUNDLE_NAME='m80-release-integrity.attestation.jsonl'
M80_RELEASE_ATTESTATION_METADATA_NAME='m80-release-attestation.json'
M80_IMAGE_KIND='minimal'
M80_SELECTOR_COLUMNS='os	arch	image_kind	bundle_name	bundle_url	bundle_sha256	size_bytes	metadata_name	metadata_sha256	checksum_name	signature_name	attestation_name	m80_version'
M80_TRUST_KEYSET_ID='github-actions-oidc:m80-release-v1'
M80_TRUST_SIGNER_WORKFLOW="${M80_PUBLIC_RELEASE_OWNER}/${M80_PUBLIC_RELEASE_REPO}/.github/workflows/release-artifacts.yml"
M80_TRUST_SIGNER_ISSUER='https://token.actions.githubusercontent.com'
M80_TRUST_VALID_FROM='2026-01-01T00:00:00Z'
M80_TRUST_VALID_UNTIL='2027-01-01T00:00:00Z'
M80_DOWNLOAD_CONNECT_TIMEOUT_SECONDS='10'
M80_DOWNLOAD_MAX_TIME_SECONDS='120'
M80_DOWNLOAD_RETRY_COUNT='2'
M80_DOWNLOAD_RETRY_DELAY_SECONDS='1'

usage() {
    cat <<EOF
usage: install.sh [m80 install options]

Downloads the m80 ${M80_RELEASE_TAG} release metadata, selects the matching
host bundle, verifies its checksum, then runs the bundled versioned m80
installer.

Common options passed through to m80 install:
  --install-root PATH  Install root, default /opt/m80
  --dry-run            Print the install plan without writing host state
  -h, --help           Print this help
EOF
}

fail() {
    echo "m80 install.sh: $*" >&2
    exit 1
}

case "${1:-}" in
    -h|--help)
        usage
        exit 0
        ;;
esac

need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "m80 install.sh: missing required tool: $1" >&2
        exit 127
    fi
}

need curl
need sha256sum
need tar
need env
need mktemp
need chmod
need mkdir
need rm
need uname
need wc
need id

validate_stable_release_tag() {
    tag=$1
    case "$tag" in
        v*) version=${tag#v} ;;
        *) fail "stable release tag must be vMAJOR.MINOR.PATCH: $tag" ;;
    esac
    major=${version%%.*}
    rest=${version#*.}
    if [ "$rest" = "$version" ]; then
        fail "stable release tag must be vMAJOR.MINOR.PATCH with no prerelease suffix: $tag"
    fi
    minor=${rest%%.*}
    patch=${rest#*.}
    if [ "$patch" = "$rest" ] || [ "${patch#*.}" != "$patch" ]; then
        fail "stable release tag must be vMAJOR.MINOR.PATCH with no prerelease suffix: $tag"
    fi
    for part in "$major" "$minor" "$patch"; do
        case "$part" in
            ''|*[!0123456789]*)
                fail "stable release tag must be vMAJOR.MINOR.PATCH with no prerelease suffix: $tag"
                ;;
        esac
    done
}

validate_stable_release_tag "$M80_RELEASE_TAG"

need python3

selected_install_root=/opt/m80
install_dry_run=no
preflight_install_args() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --dry-run)
                install_dry_run=yes
                ;;
            --install-root)
                if [ "$#" -lt 2 ]; then
                    fail "--install-root requires a path"
                fi
                selected_install_root=$2
                shift
                ;;
            --install-root=*)
                selected_install_root=${1#--install-root=}
                [ -n "$selected_install_root" ] || fail "--install-root requires a path"
                ;;
        esac
        shift
    done
}

preflight_install_args "$@"

asset_url() {
    printf '%s/%s\n' "$M80_RELEASE_BASE_URL" "$1"
}

install_requires_root() {
    [ "$install_dry_run" = no ] || return 1
    case "$selected_install_root" in
        /opt|/opt/*) return 0 ;;
        *) return 1 ;;
    esac
}

if install_requires_root && [ "$(id -u)" != 0 ]; then
    if ! command -v sudo >/dev/null 2>&1; then
        fail "missing required tool: sudo; needed to install into $selected_install_root as root; install sudo or rerun with --install-root PATH you own"
    fi
    fail "root privileges required for install-root=$selected_install_root; rerun the public installer as: curl -fsSL $(asset_url "$M80_INSTALL_NAME") | sudo sh"
fi

host_os() {
    case "$(uname -s)" in
        Linux) printf '%s\n' linux ;;
        *) fail "unsupported OS for release install: $(uname -s); report a release metadata bug or use an explicit local m80 install fixture" ;;
    esac
}

host_arch() {
    case "$(uname -m)" in
        x86_64|amd64) printf '%s\n' x86_64 ;;
        aarch64|arm64) printf '%s\n' aarch64 ;;
        *) fail "unsupported architecture for release install: $(uname -m); report a release metadata bug or use an explicit local m80 install fixture" ;;
    esac
}

integrity_retry_command() {
    printf 'curl -fsSL --connect-timeout %s --max-time %s --retry %s --retry-delay %s %s | sudo sh\n' \
        "$M80_DOWNLOAD_CONNECT_TIMEOUT_SECONDS" \
        "$M80_DOWNLOAD_MAX_TIME_SECONDS" \
        "$M80_DOWNLOAD_RETRY_COUNT" \
        "$M80_DOWNLOAD_RETRY_DELAY_SECONDS" \
        "$(asset_url "$M80_INSTALL_NAME")"
}

fail_integrity() {
    fail "$*; retry pinned command: $(integrity_retry_command)"
}

download_asset() {
    asset_name=$1
    dest=$2
    url=$(asset_url "$asset_name")
    download_release_asset "$asset_name" "$dest" "$url" "no" "plain"
}

download_integrity_asset() {
    asset_name=$1
    dest=$2
    url=$(asset_url "$asset_name")
    download_release_asset "$asset_name" "$dest" "$url" "yes" "integrity"
}

curl_failure_kind() {
    case "$1" in
        6|7) printf '%s\n' "dns_or_connect_failure" ;;
        22) printf '%s\n' "http_failure" ;;
        28) printf '%s\n' "timeout" ;;
        130) printf '%s\n' "interrupted" ;;
        *) printf '%s\n' "download_failure" ;;
    esac
}

download_release_asset() {
    asset_name=$1
    dest=$2
    url=$3
    verification_started=$4
    failure_mode=$5
    set +e
    curl -fsSL \
        --connect-timeout "$M80_DOWNLOAD_CONNECT_TIMEOUT_SECONDS" \
        --max-time "$M80_DOWNLOAD_MAX_TIME_SECONDS" \
        --retry "$M80_DOWNLOAD_RETRY_COUNT" \
        --retry-delay "$M80_DOWNLOAD_RETRY_DELAY_SECONDS" \
        "$url" \
        -o "$dest"
    status=$?
    set -e
    if [ "$status" -eq 0 ]; then
        return 0
    fi
    kind=$(curl_failure_kind "$status")
    message="failed to download release_tag=$M80_RELEASE_TAG asset=$asset_name url=$url verification_started=$verification_started failure=$kind curl_exit=$status"
    case "$failure_mode" in
        integrity) fail_integrity "$message" ;;
        *) fail "$message" ;;
    esac
}

validate_safe_token() {
    field=$1
    value=$2
    case "$value" in
        ''|*[!ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._:/+-]*)
            fail "bootstrap selector $field contains non-shell-safe characters"
            ;;
    esac
}

validate_sha256() {
    field=$1
    value=$2
    case "$value" in
        ""|*[!0123456789abcdef]*)
            fail "$field must be a lowercase sha256 digest"
            ;;
    esac
    if [ "${#value}" -ne 64 ]; then
        fail "$field must be a 64-character sha256 digest"
    fi
}

validate_positive_int() {
    field=$1
    value=$2
    case "$value" in
        ""|*[!0123456789]*)
            fail "$field must be a positive integer"
            ;;
    esac
    if [ "$value" = 0 ]; then
        fail "$field must be greater than zero"
    fi
}

validate_asset_name() {
    field=$1
    value=$2
    validate_safe_token "$field" "$value"
    case "$value" in
        .|..|*/*) fail "bootstrap selector $field must be a release asset name" ;;
    esac
}

read_checksum_digest() {
    checksum_path=$1
    asset_name=$2
    if ! read -r digest checksum_asset extra < "$checksum_path"; then
        fail "checksum sidecar missing digest for $asset_name"
    fi
    if [ -n "${extra:-}" ]; then
        fail "checksum sidecar for $asset_name has extra fields"
    fi
    validate_sha256 "checksum digest for $asset_name" "$digest"
    if [ "$checksum_asset" != "$asset_name" ]; then
        fail "checksum sidecar for $asset_name names $checksum_asset"
    fi
    printf '%s\n' "$digest"
}

verify_sha256_sidecar() {
    verify_asset_name=$1
    verify_checksum_name=$2
    verify_expected_digest=$3
    verify_checksum_path="$tmp/$verify_checksum_name"
    verify_observed_digest=$(read_checksum_digest "$verify_checksum_path" "$verify_asset_name")
    if [ "$verify_observed_digest" != "$verify_expected_digest" ]; then
        fail "checksum digest mismatch for $verify_asset_name: expected $verify_expected_digest from selector, got $verify_observed_digest from $verify_checksum_name"
    fi
    (cd "$tmp" && sha256sum -c "$verify_checksum_name" >/dev/null) || fail "checksum verification failed for $verify_asset_name"
}

verify_integrity_sha256_sidecar() {
    verify_asset_name=$1
    verify_checksum_name=$2
    verify_expected_digest=$3
    verify_checksum_path="$tmp/$verify_checksum_name"
    verify_observed_digest=$(read_checksum_digest "$verify_checksum_path" "$verify_asset_name")
    if [ "$verify_observed_digest" != "$verify_expected_digest" ]; then
        fail_integrity "checksum digest mismatch for $verify_asset_name: expected $verify_expected_digest, got $verify_observed_digest from $verify_checksum_name"
    fi
    (cd "$tmp" && sha256sum -c "$verify_checksum_name" >/dev/null) || fail_integrity "checksum verification failed for $verify_asset_name"
}

parse_bootstrap_selector() {
    selector_path=$1
    requested_os=$2
    requested_arch=$3
    requested_image_kind=$4
    tab=$(printf '\t')
    expected_schema="schema_version${tab}1"
    expected_tag="release_tag${tab}${M80_RELEASE_TAG}"
    expected_columns="columns${tab}${M80_SELECTOR_COLUMNS}"
    line_no=0
    match_count=0
    selected_bundle_name=
    selected_bundle_url=
    selected_bundle_sha256=
    selected_size_bytes=
    selected_metadata_name=
    selected_metadata_sha256=
    selected_checksum_name=
    selected_signature_name=
    selected_attestation_name=
    selected_m80_version=
    available_tuples=

    while IFS= read -r line || [ -n "$line" ]; do
        line_no=$((line_no + 1))
        case "$line_no" in
            1)
                [ "$line" = "$expected_schema" ] || fail "unsupported bootstrap selector schema in $(asset_url "$M80_BOOTSTRAP_SELECTOR_NAME")"
                continue
                ;;
            2)
                [ "$line" = "$expected_tag" ] || fail "bootstrap selector release_tag mismatch for $M80_RELEASE_TAG from $(asset_url "$M80_BOOTSTRAP_SELECTOR_NAME")"
                continue
                ;;
            3)
                [ "$line" = "$expected_columns" ] || fail "bootstrap selector columns mismatch from $(asset_url "$M80_BOOTSTRAP_SELECTOR_NAME")"
                continue
                ;;
        esac

        old_ifs=$IFS
        IFS=$tab
        # shellcheck disable=SC2086
        set -- $line
        IFS=$old_ifs

        [ "$#" -eq 14 ] || fail "bootstrap selector row shape invalid at line $line_no"
        [ "$1" = row ] || fail "bootstrap selector row marker invalid at line $line_no"
        for value in "$@"; do
            validate_safe_token "row value at line $line_no" "$value"
        done

        row_os=$2
        row_arch=$3
        row_image_kind=$4
        row_tuple="${row_os}/${row_arch}/${row_image_kind}"
        case " $available_tuples " in
            *" $row_tuple "*) ;;
            *) available_tuples="${available_tuples}${available_tuples:+ }${row_tuple}" ;;
        esac
        if [ "$row_os" = "$requested_os" ] && [ "$row_arch" = "$requested_arch" ] && [ "$row_image_kind" = "$requested_image_kind" ]; then
            match_count=$((match_count + 1))
            [ "$match_count" -eq 1 ] || fail "bootstrap selector duplicate tuple for ${requested_os}/${requested_arch}/${requested_image_kind}"
            selected_bundle_name=$5
            selected_bundle_url=$6
            selected_bundle_sha256=$7
            selected_size_bytes=$8
            selected_metadata_name=$9
            selected_metadata_sha256=${10}
            selected_checksum_name=${11}
            selected_signature_name=${12}
            selected_attestation_name=${13}
            selected_m80_version=${14}
        fi
    done < "$selector_path"

    [ "$line_no" -ge 3 ] || fail "bootstrap selector missing header rows from $(asset_url "$M80_BOOTSTRAP_SELECTOR_NAME")"
    [ "$match_count" -eq 1 ] || fail "bootstrap selector missing tuple for release_tag=${M80_RELEASE_TAG} os=${requested_os} arch=${requested_arch} image_kind=${requested_image_kind} available_tuples=${available_tuples:-none} selector=$(asset_url "$M80_BOOTSTRAP_SELECTOR_NAME") index=$(asset_url "$M80_ASSET_INDEX_NAME")"

    validate_sha256 "bootstrap selector bundle_sha256" "$selected_bundle_sha256"
    validate_positive_int "bootstrap selector size_bytes" "$selected_size_bytes"
    validate_asset_name "bundle_name" "$selected_bundle_name"
    validate_asset_name "metadata_name" "$selected_metadata_name"
    validate_sha256 "bootstrap selector metadata_sha256" "$selected_metadata_sha256"
    validate_asset_name "checksum_name" "$selected_checksum_name"
    expected_bundle_url=$(asset_url "$selected_bundle_name")
    if [ "$selected_bundle_url" != "$expected_bundle_url" ]; then
        fail "bootstrap selector bundle_url mismatch for ${requested_os}/${requested_arch}/${requested_image_kind}: expected $expected_bundle_url, got $selected_bundle_url"
    fi
    if [ "$selected_checksum_name" != "$selected_bundle_name.sha256" ]; then
        fail "bootstrap selector checksum_name mismatch for $selected_bundle_name"
    fi
    if [ "$selected_signature_name" != "-" ]; then
        fail "bootstrap selector signature_name names unsupported detached signature $selected_signature_name for $selected_bundle_name"
    fi
    validate_asset_name "attestation_name" "$selected_attestation_name"
    if [ "$selected_attestation_name" != "$M80_RELEASE_ATTESTATION_BUNDLE_NAME" ]; then
        fail "bootstrap selector attestation_name mismatch for $selected_bundle_name: expected $M80_RELEASE_ATTESTATION_BUNDLE_NAME, got $selected_attestation_name"
    fi
    if [ "$selected_m80_version" != "$M80_RELEASE_TAG" ]; then
        fail "bootstrap selector m80_version mismatch for $selected_bundle_name: expected $M80_RELEASE_TAG, got $selected_m80_version"
    fi
}

validate_release_integrity_material() {
    phase=$1
    facts_path=$2
    python3 - \
        "$phase" \
        "$tmp" \
        "$M80_RELEASE_TAG" \
        "${M80_PUBLIC_RELEASE_OWNER}/${M80_PUBLIC_RELEASE_REPO}" \
        "$(asset_url "$M80_BOOTSTRAP_SELECTOR_NAME")" \
        "$(asset_url "$M80_ASSET_INDEX_NAME")" \
        "$host_os_value" \
        "$host_arch_value" \
        "$M80_IMAGE_KIND" \
        "$selected_bundle_name" \
        "$selected_bundle_url" \
        "$selected_bundle_sha256" \
        "$selected_size_bytes" \
        "$selected_metadata_name" \
        "$selected_metadata_sha256" \
        "$selected_checksum_name" \
        "$selected_signature_name" \
        "$selected_attestation_name" \
        "$M80_TRUST_KEYSET_ID" \
        "$M80_TRUST_SIGNER_WORKFLOW" \
        "$M80_TRUST_SIGNER_ISSUER" \
        "$M80_TRUST_VALID_FROM" \
        "$M80_TRUST_VALID_UNTIL" > "$facts_path" <<'PY'
from __future__ import annotations

from datetime import datetime, timezone
import base64
import hashlib
import json
from pathlib import Path
import re
import sys

(
    phase,
    tmp_dir,
    release_tag,
    repository,
    selector_url,
    index_url,
    host_os,
    host_arch,
    image_kind,
    bundle_name,
    bundle_url,
    bundle_sha256,
    bundle_size,
    metadata_name,
    metadata_sha256,
    checksum_name,
    signature_name,
    attestation_name,
    trust_keyset_id,
    trust_signer_workflow,
    trust_signer_issuer,
    trust_valid_from,
    trust_valid_until,
) = sys.argv[1:]

tmp = Path(tmp_dir)
BUILD_MANIFEST_SCHEMA_VERSION = 1
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
APT_PACKAGE_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9+.-]*$")
OCI_SHA256_DIGEST_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
MECHANISM = "github-artifact-attestation"
INTEGRITY_NAME = "m80-release-integrity.json"
ATTESTATION_METADATA_NAME = "m80-release-attestation.json"
INSTALL_NAME = "install.sh"
ASSET_INDEX_NAME = "m80-release-assets.json"
BOOTSTRAP_SELECTOR_NAME = "m80-bootstrap-selector.tsv"
BUILD_MANIFEST_NAME = "m80-release-build.json"
PUBLIC_SHA256SUMS_NAME = "SHA256SUMS"
EXPECTED_INTEGRITY_FIELDS = {
    "schema_version",
    "mechanism",
    "repository",
    "release_tag",
    "commit_sha",
    "target",
    "rust_toolchain",
    "m80_package_version",
    "bundle_metadata_name",
    "bundle_metadata_sha256",
    "subjects",
}
EXPECTED_ATTESTATION_FIELDS = {
    "schema_version",
    "mechanism",
    "repository",
    "release_tag",
    "predicate_sha256",
    "signer_identity",
    "issuer",
    "keyset_id",
    "certificate_not_before",
    "certificate_not_after",
}
EXPECTED_INDEX_FIELDS = {"schema_version", "release_tag", "assets"}
EXPECTED_ASSET_FIELDS = {
    "name",
    "url",
    "sha256",
    "size_bytes",
    "metadata_name",
    "metadata_sha256",
    "checksum_name",
    "signature_name",
    "attestation_name",
    "target",
    "os",
    "arch",
    "image_kind",
    "release_tag",
    "m80_version",
    "guest_protocol_version",
    "manifest_schema_version",
    "expected_firecracker_version",
}
EXPECTED_SUBJECT_FIELDS = {"name", "kind", "sha256", "size_bytes"}
EXPECTED_BUILD_MANIFEST_FIELDS = {
    "schema_version",
    "release_tag",
    "source_commit",
    "rust_toolchain",
    "target",
    "target_triples",
    "m80_package_version",
    "image_kind",
    "cargo_lock_sha256",
    "builder_identity",
    "builder_os_image",
    "apt_packages",
    "container_digest",
    "bundle_metadata_name",
    "bundle_metadata_sha256",
}
EXPECTED_APT_PACKAGE_FIELDS = {"name", "version"}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def read_json(path: Path, label: str) -> dict:
    require(path.is_file(), f"{label} missing: {path.name}")
    with path.open() as f:
        payload = json.load(f)
    require(isinstance(payload, dict), f"{label} must be a JSON object")
    return payload


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_time(value: str, label: str) -> datetime:
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise SystemExit(f"{label} must be RFC3339") from exc
    require(parsed.tzinfo is not None, f"{label} must include timezone")
    return parsed.astimezone(timezone.utc)


def require_exact_fields(payload: dict, expected: set[str], label: str) -> None:
    actual = set(payload)
    missing = sorted(expected - actual)
    extra = sorted(actual - expected)
    require(not missing, f"{label} missing field(s): {', '.join(missing)}")
    require(not extra, f"{label} has unknown field(s): {', '.join(extra)}")


def require_sha(value: object, label: str) -> str:
    require(isinstance(value, str) and SHA256_RE.match(value) is not None, f"{label} must be lowercase sha256")
    return value


def require_size(value: object, label: str) -> int:
    require(isinstance(value, int) and value > 0, f"{label} must be a positive integer")
    return value


def require_nonempty_str(payload: dict, key: str, label: str) -> str:
    value = payload.get(key)
    require(isinstance(value, str) and value, f"{label} {key} must be a nonempty string")
    return value


def verify_top_level(material: dict) -> str:
    require_exact_fields(material, EXPECTED_INTEGRITY_FIELDS, "release integrity material")
    require(material["schema_version"] == 1, "unsupported release integrity schema_version")
    require(
        material["mechanism"] == MECHANISM,
        f"release integrity mechanism mismatch: expected {MECHANISM}, got {material['mechanism']}",
    )
    require(
        material["repository"] == repository,
        f"release integrity repository mismatch: expected {repository}, got {material['repository']}",
    )
    require(
        material["release_tag"] == release_tag,
        f"release integrity release_tag mismatch: expected {release_tag}, got {material['release_tag']}",
    )
    commit_sha = material["commit_sha"]
    require(isinstance(commit_sha, str) and COMMIT_RE.match(commit_sha) is not None, "release integrity commit_sha invalid")
    target = f"{host_os}-{host_arch}"
    require(material["target"] == target, f"release integrity target mismatch: expected {target}, got {material['target']}")
    require(
        material["bundle_metadata_name"] == metadata_name,
        f"release integrity bundle_metadata_name mismatch: expected {metadata_name}, got {material['bundle_metadata_name']}",
    )
    require_sha(material["bundle_metadata_sha256"], "release integrity bundle_metadata_sha256")
    require(
        material["bundle_metadata_sha256"] == metadata_sha256,
        "release integrity bundle_metadata_sha256 mismatch with bootstrap selector",
    )
    return commit_sha


def verify_attestation_metadata(material_path: Path, material: dict) -> None:
    attestation = read_json(tmp / ATTESTATION_METADATA_NAME, "release attestation metadata")
    require_exact_fields(attestation, EXPECTED_ATTESTATION_FIELDS, "release attestation metadata")
    require(attestation["schema_version"] == 1, "unsupported release attestation metadata schema_version")
    require(attestation["mechanism"] == MECHANISM, "release attestation mechanism mismatch")
    require(attestation["repository"] == repository, "release trust attestation repository mismatch")
    require(attestation["repository"] == material["repository"], "release trust predicate repository mismatch")
    require(attestation["release_tag"] == release_tag, "release trust attestation release_tag mismatch")
    require(attestation["release_tag"] == material["release_tag"], "release trust predicate tag mismatch")
    require_sha(attestation["predicate_sha256"], "release attestation predicate_sha256")
    require(attestation["predicate_sha256"] == sha256_file(material_path), "release trust predicate digest mismatch")
    require(attestation["signer_identity"] == trust_signer_workflow, "release trust signer not allowed")
    require(attestation["issuer"] == trust_signer_issuer, "release trust issuer mismatch")
    require(attestation["keyset_id"] == trust_keyset_id, "release trust keyset mismatch")
    now = datetime.now(timezone.utc)
    trust_from = parse_time(trust_valid_from, "release trust valid_from")
    trust_until = parse_time(trust_valid_until, "release trust valid_until")
    cert_from = parse_time(attestation["certificate_not_before"], "release attestation certificate_not_before")
    cert_until = parse_time(attestation["certificate_not_after"], "release attestation certificate_not_after")
    require(trust_from <= trust_until, "release trust validity window is inverted")
    require(cert_from <= cert_until, "release attestation certificate window is inverted")
    require(trust_from <= now, "release trust policy is not active yet")
    require(now <= trust_until, "release trust policy expired")
    require(cert_from <= now, "release trust certificate is not active yet")
    require(now <= cert_until, "release trust certificate expired")


def pointer(payload: dict, parts: list[str], label: str):
    current = payload
    for part in parts:
        require(isinstance(current, dict) and part in current, f"{label} missing")
        current = current[part]
    return current


def require_equal(actual: object, expected: object, label: str) -> None:
    require(actual == expected, f"{label} mismatch: expected {expected}, got {actual}")


def verify_attestation_bundle(material_path: Path, material: dict) -> None:
    bundle = read_json(tmp / attestation_name, "release attestation bundle")
    require_equal(
        bundle.get("mediaType"),
        "application/vnd.dev.sigstore.bundle.v0.3+json",
        "release attestation bundle mediaType",
    )
    require(pointer(bundle, ["verificationMaterial", "certificate", "rawBytes"], "release attestation bundle certificate rawBytes"), "release attestation bundle certificate rawBytes empty")
    tlog_entries = pointer(bundle, ["verificationMaterial", "tlogEntries"], "release attestation bundle tlogEntries")
    require(isinstance(tlog_entries, list) and tlog_entries, "release attestation bundle tlogEntries missing or empty")
    signatures = pointer(bundle, ["dsseEnvelope", "signatures"], "release attestation bundle signatures")
    require(isinstance(signatures, list) and signatures, "release attestation bundle signatures missing or empty")
    require_equal(
        pointer(bundle, ["dsseEnvelope", "payloadType"], "release attestation bundle payloadType"),
        "application/vnd.in-toto+json",
        "release attestation bundle payloadType",
    )
    payload_b64 = pointer(bundle, ["dsseEnvelope", "payload"], "release attestation bundle payload")
    require(isinstance(payload_b64, str) and payload_b64, "release attestation bundle payload empty")
    try:
        statement = json.loads(base64.b64decode(payload_b64))
    except Exception as exc:
        raise SystemExit(f"release attestation bundle statement invalid: {exc}") from exc
    require_equal(statement.get("_type"), "https://in-toto.io/Statement/v1", "release attestation statement type")
    require_equal(statement.get("predicateType"), "https://slsa.dev/provenance/v1", "release attestation predicateType")
    subject_ok = False
    for subject in statement.get("subject", []):
        digest = subject.get("digest") if isinstance(subject, dict) else None
        if (
            isinstance(digest, dict)
            and subject.get("name") in {str(material_path), material_path.name}
            and digest.get("sha256") == sha256_file(material_path)
        ):
            subject_ok = True
            break
    require(subject_ok, "release attestation statement omitted release-integrity predicate name/sha256 subject")
    workflow = pointer(statement, ["predicate", "buildDefinition", "externalParameters", "workflow"], "release attestation workflow")
    require_equal(workflow.get("repository"), "https://github.com/moradology/m80", "release attestation workflow repository")
    require_equal(workflow.get("path"), ".github/workflows/release-artifacts.yml", "release attestation workflow path")
    require_equal(workflow.get("ref"), f"refs/tags/{release_tag}", "release attestation workflow ref")
    runner = pointer(statement, ["predicate", "buildDefinition", "internalParameters", "github", "runner_environment"], "release attestation runner environment")
    require_equal(runner, "github-hosted", "release attestation runner environment")
    builder = pointer(statement, ["predicate", "runDetails", "builder", "id"], "release attestation builder id")
    require_equal(builder, f"https://github.com/moradology/m80/.github/workflows/release-artifacts.yml@refs/tags/{release_tag}", "release attestation builder id")
    dependencies = pointer(statement, ["predicate", "buildDefinition", "resolvedDependencies"], "release attestation resolvedDependencies")
    expected_uri = f"git+https://github.com/moradology/m80@refs/tags/{release_tag}"
    require(isinstance(dependencies, list), "release attestation resolvedDependencies must be a list")
    for dependency in dependencies:
        digest = dependency.get("digest") if isinstance(dependency, dict) else None
        if dependency.get("uri") == expected_uri and isinstance(digest, dict) and digest.get("gitCommit") == material["commit_sha"]:
            return
    raise SystemExit(
        "release attestation resolved dependency mismatch: "
        f"expected_uri={expected_uri} expected_commit={material['commit_sha']}"
    )


def verify_metadata(material: dict) -> dict:
    metadata_path = tmp / metadata_name
    metadata = read_json(metadata_path, "bundle metadata")
    require(sha256_file(metadata_path) == metadata_sha256, f"release integrity sha256 mismatch for {metadata_name}")
    require(metadata["release_tag"] == release_tag, "release integrity bundle metadata release_tag mismatch")
    require(metadata["release_tag"] == material["release_tag"], "release integrity bundle metadata predicate tag mismatch")
    require(metadata["m80_version"] == release_tag, "release integrity bundle metadata m80_version mismatch")
    require(metadata["package_version"] == material["m80_package_version"], "release integrity bundle metadata package_version mismatch")
    require(metadata["target"] == material["target"], "release integrity bundle metadata target mismatch")
    return metadata


def verify_build_manifest(material: dict, metadata: dict) -> None:
    manifest = read_json(tmp / BUILD_MANIFEST_NAME, "build manifest")
    require_exact_fields(manifest, EXPECTED_BUILD_MANIFEST_FIELDS, "build manifest")
    require(manifest["schema_version"] == BUILD_MANIFEST_SCHEMA_VERSION, "unsupported build manifest schema_version")
    require(manifest["release_tag"] == release_tag, "build manifest release_tag mismatch")
    require(manifest["release_tag"] == material["release_tag"], "build manifest predicate release_tag mismatch")
    require(manifest["source_commit"] == material["commit_sha"], "build manifest source_commit mismatch")
    require(manifest["rust_toolchain"] == material["rust_toolchain"], "build manifest rust_toolchain mismatch")
    target = f"{host_os}-{host_arch}"
    require(manifest["target"] == target, f"build manifest target mismatch: expected {target}, got {manifest['target']}")
    require(manifest["target"] == material["target"], "build manifest predicate target mismatch")
    require(
        manifest["m80_package_version"] == material["m80_package_version"],
        "build manifest m80_package_version mismatch",
    )
    require(
        manifest["m80_package_version"] == metadata["package_version"],
        "build manifest bundle metadata package_version mismatch",
    )
    require(manifest["image_kind"] == image_kind, "build manifest image_kind mismatch")
    require(manifest["image_kind"] == metadata["image_kind"], "build manifest metadata image_kind mismatch")
    require(manifest["bundle_metadata_name"] == metadata_name, "build manifest bundle_metadata_name mismatch")
    require(
        manifest["bundle_metadata_sha256"] == metadata_sha256,
        "build manifest bundle_metadata_sha256 mismatch",
    )
    require(
        manifest["bundle_metadata_sha256"] == material["bundle_metadata_sha256"],
        "build manifest predicate bundle_metadata_sha256 mismatch",
    )
    require_sha(manifest["cargo_lock_sha256"], "build manifest cargo_lock_sha256")
    verify_build_manifest_target_triples(manifest["target_triples"])
    require_nonempty_str(manifest, "builder_identity", "build manifest")
    require_nonempty_str(manifest, "builder_os_image", "build manifest")
    verify_build_manifest_builder_material(manifest)


def verify_build_manifest_target_triples(value: object) -> None:
    require(isinstance(value, list) and value, "build manifest target_triples must be a non-empty list")
    seen: set[str] = set()
    for triple in value:
        require(isinstance(triple, str) and triple, "build manifest target triple must not be empty")
        require(
            not any(ch.isspace() or ord(ch) < 0x20 or ord(ch) == 0x7F for ch in triple),
            f"build manifest target triple invalid: {triple!r}",
        )
        require(triple not in seen, f"build manifest duplicate target triple: {triple}")
        seen.add(triple)
    require(
        "x86_64-unknown-linux-musl" in seen,
        "build manifest target_triples missing x86_64-unknown-linux-musl",
    )


def verify_build_manifest_builder_material(manifest: dict) -> None:
    apt_packages = manifest["apt_packages"]
    require(isinstance(apt_packages, list), "build manifest apt_packages must be a list")
    seen: set[str] = set()
    for package in apt_packages:
        require(isinstance(package, dict), "build manifest apt package must be an object")
        require_exact_fields(package, EXPECTED_APT_PACKAGE_FIELDS, "build manifest apt package")
        name = package.get("name")
        version = package.get("version")
        require(isinstance(name, str) and APT_PACKAGE_NAME_RE.fullmatch(name) is not None, "build manifest apt package name invalid")
        require(isinstance(version, str) and version, f"build manifest apt package version missing for {name}")
        require(
            not any(ch.isspace() or ord(ch) < 0x20 or ord(ch) == 0x7F for ch in version),
            f"build manifest apt package version invalid for {name}",
        )
        require(name not in seen, f"build manifest duplicate apt package: {name}")
        seen.add(name)
    container_digest = manifest["container_digest"]
    require(
        container_digest is None
        or (isinstance(container_digest, str) and OCI_SHA256_DIGEST_RE.fullmatch(container_digest) is not None),
        "build manifest container_digest invalid",
    )
    require(apt_packages or container_digest is not None, "build manifest missing apt packages or container digest")


def verify_asset_index(metadata: dict) -> dict:
    index = read_json(tmp / ASSET_INDEX_NAME, "asset index")
    require_exact_fields(index, EXPECTED_INDEX_FIELDS, "asset index")
    require(index["schema_version"] == 1, "release integrity unsupported asset index schema_version")
    require(index["release_tag"] == release_tag, "release integrity asset index release_tag mismatch")
    require(isinstance(index["assets"], list) and index["assets"], "release integrity asset index assets must not be empty")
    selected = None
    seen: set[tuple[str, str, str]] = set()
    for asset in index["assets"]:
        require(isinstance(asset, dict), "release integrity asset index asset must be an object")
        name = asset.get("name") if isinstance(asset.get("name"), str) else "<unknown>"
        require_exact_fields(asset, EXPECTED_ASSET_FIELDS, f"asset index asset {name}")
        tuple_key = (asset["os"], asset["arch"], asset["image_kind"])
        require(tuple_key not in seen, f"release integrity asset index duplicate tuple: {'/'.join(tuple_key)}")
        seen.add(tuple_key)
        if tuple_key == (host_os, host_arch, image_kind):
            require(selected is None, f"release integrity asset index duplicate tuple: {host_os}/{host_arch}/{image_kind}")
            selected = asset
    require(selected is not None, f"release integrity asset index missing tuple: {host_os}/{host_arch}/{image_kind}")
    expected = {
        "name": bundle_name,
        "url": bundle_url,
        "sha256": bundle_sha256,
        "size_bytes": int(bundle_size),
        "metadata_name": metadata_name,
        "metadata_sha256": metadata_sha256,
        "checksum_name": checksum_name,
        "signature_name": None if signature_name == "-" else signature_name,
        "attestation_name": attestation_name,
        "target": f"{host_os}-{host_arch}",
        "os": host_os,
        "arch": host_arch,
        "image_kind": image_kind,
        "release_tag": release_tag,
        "m80_version": metadata["m80_version"],
        "guest_protocol_version": metadata["guest_protocol_version"],
        "manifest_schema_version": metadata["manifest_schema_version"],
        "expected_firecracker_version": metadata["expected_firecracker_version"],
    }
    for field, expected_value in expected.items():
        require(
            selected[field] == expected_value,
            "release integrity selector/index "
            f"{field} mismatch for release_tag={release_tag} "
            f"os={host_os} arch={host_arch} image_kind={image_kind} "
            f"selector_url={selector_url} index_url={index_url} "
            f"expected_from_selector={expected_value} observed_in_index={selected[field]}",
        )
    return index


def expected_subjects_from_index(index: dict) -> dict[str, str]:
    expected = {
        INSTALL_NAME: "installer",
        f"{INSTALL_NAME}.sha256": "checksum-sidecar",
        ASSET_INDEX_NAME: "asset-index",
        f"{ASSET_INDEX_NAME}.sha256": "checksum-sidecar",
        BOOTSTRAP_SELECTOR_NAME: "bootstrap-selector",
        f"{BOOTSTRAP_SELECTOR_NAME}.sha256": "checksum-sidecar",
        BUILD_MANIFEST_NAME: "build-manifest",
        f"{BUILD_MANIFEST_NAME}.sha256": "checksum-sidecar",
        PUBLIC_SHA256SUMS_NAME: "checksum-manifest",
    }
    for asset in index["assets"]:
        add_expected_subject(expected, asset["name"], "release-bundle")
        add_expected_subject(expected, asset["checksum_name"], "checksum-sidecar")
        add_expected_subject(expected, asset["metadata_name"], "bundle-metadata")
        add_expected_subject(expected, f"{asset['metadata_name']}.sha256", "checksum-sidecar")
        if asset["signature_name"] is not None:
            add_expected_subject(expected, asset["signature_name"], "detached-signature")
    return expected


def add_expected_subject(subjects: dict[str, str], name: str, kind: str) -> None:
    require(name not in subjects, f"release integrity duplicate expected subject {name}")
    subjects[name] = kind


def downloaded_subjects() -> set[str]:
    names = {
        INSTALL_NAME,
        f"{INSTALL_NAME}.sha256",
        metadata_name,
        f"{metadata_name}.sha256",
        ASSET_INDEX_NAME,
        f"{ASSET_INDEX_NAME}.sha256",
        BOOTSTRAP_SELECTOR_NAME,
        f"{BOOTSTRAP_SELECTOR_NAME}.sha256",
        BUILD_MANIFEST_NAME,
        f"{BUILD_MANIFEST_NAME}.sha256",
        PUBLIC_SHA256SUMS_NAME,
    }
    if phase == "full":
        names.update({bundle_name, checksum_name})
    return names


def verify_subjects(material: dict, index: dict) -> str:
    subjects = material["subjects"]
    require(isinstance(subjects, list), "release integrity subjects must be a list")
    expected = expected_subjects_from_index(index)
    downloaded = downloaded_subjects()
    by_name: dict[str, dict] = {}
    for subject in subjects:
        require(isinstance(subject, dict), "release integrity subject must be an object")
        name = subject.get("name")
        require(isinstance(name, str) and name, "release integrity subject missing name")
        require(name not in by_name, f"release integrity duplicate subject {name}")
        require_exact_fields(subject, EXPECTED_SUBJECT_FIELDS, f"release integrity subject {name}")
        kind = subject["kind"]
        require(expected.get(name) is not None, f"release integrity unexpected subject {name}")
        require(kind == expected[name], f"release integrity subject {name} kind mismatch")
        require_sha(subject["sha256"], f"release integrity subject {name} sha256")
        require_size(subject["size_bytes"], f"release integrity subject {name} size_bytes")
        by_name[name] = subject
    missing = sorted(set(expected) - set(by_name))
    require(not missing, "release integrity missing subject(s): " + ", ".join(missing))
    extra = sorted(set(by_name) - set(expected))
    require(not extra, "release integrity unexpected subject(s): " + ", ".join(extra))
    for name, subject in by_name.items():
        if name not in downloaded:
            continue
        path = tmp / name
        require(path.is_file(), f"release integrity subject file missing: {name}")
        actual_sha = sha256_file(path)
        require(
            actual_sha == subject["sha256"],
            f"release integrity sha256 mismatch for {name}: expected {subject['sha256']}, got {actual_sha}",
        )
        actual_size = path.stat().st_size
        require(
            actual_size == subject["size_bytes"],
            f"release integrity size mismatch for {name}: expected {subject['size_bytes']}, got {actual_size}",
        )
    return by_name[INSTALL_NAME]["sha256"]


require(phase in {"prebundle", "full"}, f"unsupported release integrity verification phase: {phase}")
material_path = tmp / INTEGRITY_NAME
material = read_json(material_path, "release integrity material")
commit_sha = verify_top_level(material)
verify_attestation_metadata(material_path, material)
verify_attestation_bundle(material_path, material)
metadata = verify_metadata(material)
verify_build_manifest(material, metadata)
asset_index = verify_asset_index(metadata)
install_sha256 = verify_subjects(material, asset_index)
print(f"commit_sha={commit_sha}")
print(f"install_sha256={install_sha256}")
PY
}

verify_extracted_m80_identity() {
    binary_path=$1
    identity_path="$tmp/extracted-m80-version.json"
    if ! "$binary_path" --json version > "$identity_path"; then
        fail_integrity "extracted m80 identity command failed for $binary_path"
    fi
    python3 - "$identity_path" "$metadata_path" "$build_manifest_path" "$M80_RELEASE_TAG" "$commit_sha" <<'PY' || fail_integrity "extracted m80 identity verification failed"
from __future__ import annotations

import json
from pathlib import Path
import sys


identity_path, metadata_path, build_manifest_path, release_tag, source_commit = sys.argv[1:]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def read_json(path: str, label: str) -> dict:
    with Path(path).open() as f:
        payload = json.load(f)
    require(isinstance(payload, dict), f"{label} must be a JSON object")
    return payload


def require_equal(actual: object, expected: object, label: str) -> None:
    require(actual == expected, f"{label} mismatch: expected {expected!r}, got {actual!r}")


identity = read_json(identity_path, "extracted m80 identity")
metadata = read_json(metadata_path, "bundle metadata")
build_manifest = read_json(build_manifest_path, "build manifest")

require_equal(identity.get("version"), 1, "extracted m80 identity envelope version")
data = identity.get("data")
require(isinstance(data, dict), "extracted m80 identity data must be a JSON object")
required_fields = {
    "binary_version",
    "package_version",
    "release_tag",
    "release_build",
    "version_status",
    "expected_release_tag",
    "source_commit",
    "target",
    "target_triple",
    "protocol_version",
    "manifest_schema_version",
    "build_receipt_schema_version",
    "install_provenance_schema_version",
}
missing = sorted(field for field in required_fields if field not in data)
require(not missing, "extracted m80 identity missing field(s): " + ", ".join(missing))

require_equal(data["version_status"], "release", "extracted m80 version_status")
require_equal(data["release_build"], True, "extracted m80 release_build")
require_equal(data["binary_version"], release_tag, "extracted m80 binary_version")
require_equal(data["release_tag"], release_tag, "extracted m80 release_tag")
require_equal(data["expected_release_tag"], release_tag, "extracted m80 expected_release_tag")
require_equal(data["source_commit"], source_commit, "extracted m80 source_commit")
require_equal(build_manifest.get("source_commit"), source_commit, "build manifest source_commit")
require_equal(metadata.get("release_tag"), release_tag, "bundle metadata release_tag")
require_equal(metadata.get("m80_version"), release_tag, "bundle metadata m80_version")
require_equal(data["target"], metadata.get("target"), "extracted m80 target")
require_equal(data["target"], build_manifest.get("target"), "extracted m80 build target")
target_triples = build_manifest.get("target_triples")
require(isinstance(target_triples, list), "build manifest target_triples must be a JSON array")
require(isinstance(data["target_triple"], str) and data["target_triple"], "extracted m80 target_triple must be a string")
require(
    data["target_triple"] in target_triples,
    f"extracted m80 target_triple missing from build manifest target_triples: {data['target_triple']!r}",
)
require_equal(data["package_version"], metadata.get("package_version"), "extracted m80 package_version")
require_equal(data["package_version"], build_manifest.get("m80_package_version"), "extracted m80 build package_version")
require_equal(data["protocol_version"], metadata.get("m80_protocol_version"), "extracted m80 protocol_version")
require_equal(data["protocol_version"], metadata.get("guest_protocol_version"), "extracted m80 guest protocol_version")
require_equal(data["manifest_schema_version"], metadata.get("manifest_schema_version"), "extracted m80 manifest_schema_version")
require_equal(
    data["build_receipt_schema_version"],
    metadata.get("build_receipt_schema_version"),
    "extracted m80 build_receipt_schema_version",
)
require_equal(
    data["install_provenance_schema_version"],
    metadata.get("install_provenance_schema_version"),
    "extracted m80 install_provenance_schema_version",
)

print(
    "m80 install.sh: verified handoff binary="
    f"{data['binary_version']} source_commit={data['source_commit']} "
    f"target={data['target']} target_triple={data['target_triple']} "
    f"protocol={data['protocol_version']} manifest_schema={data['manifest_schema_version']}",
    file=sys.stderr,
)
PY
}

tmp="$(mktemp -d "${TMPDIR:-/tmp}/m80-install.XXXXXX")"
cleanup() {
    rm -rf "$tmp"
}
trap cleanup EXIT HUP INT TERM

host_os_value=$(host_os)
host_arch_value=$(host_arch)
selector_path="$tmp/$M80_BOOTSTRAP_SELECTOR_NAME"
selector_checksum_path="$tmp/$M80_BOOTSTRAP_SELECTOR_NAME.sha256"
index_path="$tmp/$M80_ASSET_INDEX_NAME"
index_checksum_path="$tmp/$M80_ASSET_INDEX_NAME.sha256"
integrity_path="$tmp/$M80_RELEASE_INTEGRITY_NAME"
attestation_bundle_path="$tmp/$M80_RELEASE_ATTESTATION_BUNDLE_NAME"
attestation_metadata_path="$tmp/$M80_RELEASE_ATTESTATION_METADATA_NAME"
install_path="$tmp/$M80_INSTALL_NAME"
install_checksum_path="$tmp/$M80_INSTALL_NAME.sha256"
build_manifest_path="$tmp/$M80_RELEASE_BUILD_NAME"
build_manifest_checksum_path="$tmp/$M80_RELEASE_BUILD_NAME.sha256"
public_sha256s_path="$tmp/$M80_PUBLIC_SHA256SUMS_NAME"
integrity_facts_path="$tmp/release-integrity.env"
extract_dir="$tmp/extract"

echo "m80 install.sh: release=$M80_RELEASE_TAG" >&2
echo "m80 install.sh: host=${host_os_value}/${host_arch_value} image_kind=$M80_IMAGE_KIND" >&2
echo "m80 install.sh: selector=$(asset_url "$M80_BOOTSTRAP_SELECTOR_NAME")" >&2
echo "m80 install.sh: index=$(asset_url "$M80_ASSET_INDEX_NAME")" >&2

download_asset "$M80_BOOTSTRAP_SELECTOR_NAME" "$selector_path"
download_asset "$M80_BOOTSTRAP_SELECTOR_NAME.sha256" "$selector_checksum_path"
selector_digest=$(read_checksum_digest "$selector_checksum_path" "$M80_BOOTSTRAP_SELECTOR_NAME")
verify_sha256_sidecar "$M80_BOOTSTRAP_SELECTOR_NAME" "$M80_BOOTSTRAP_SELECTOR_NAME.sha256" "$selector_digest"

download_asset "$M80_ASSET_INDEX_NAME" "$index_path"
download_asset "$M80_ASSET_INDEX_NAME.sha256" "$index_checksum_path"
index_digest=$(read_checksum_digest "$index_checksum_path" "$M80_ASSET_INDEX_NAME")
verify_sha256_sidecar "$M80_ASSET_INDEX_NAME" "$M80_ASSET_INDEX_NAME.sha256" "$index_digest"

parse_bootstrap_selector "$selector_path" "$host_os_value" "$host_arch_value" "$M80_IMAGE_KIND"

bundle_path="$tmp/$selected_bundle_name"
checksum_path="$tmp/$selected_checksum_name"
metadata_path="$tmp/$selected_metadata_name"
metadata_checksum_path="$tmp/$selected_metadata_name.sha256"

echo "m80 install.sh: bundle=$selected_bundle_url" >&2

download_integrity_asset "$M80_RELEASE_INTEGRITY_NAME" "$integrity_path"
download_integrity_asset "$selected_attestation_name" "$attestation_bundle_path"
download_integrity_asset "$M80_RELEASE_ATTESTATION_METADATA_NAME" "$attestation_metadata_path"
download_integrity_asset "$selected_metadata_name" "$metadata_path"
download_integrity_asset "$selected_metadata_name.sha256" "$metadata_checksum_path"
verify_integrity_sha256_sidecar "$selected_metadata_name" "$selected_metadata_name.sha256" "$selected_metadata_sha256"
download_integrity_asset "$M80_INSTALL_NAME" "$install_path"
download_integrity_asset "$M80_INSTALL_NAME.sha256" "$install_checksum_path"
install_sidecar_digest=$(read_checksum_digest "$install_checksum_path" "$M80_INSTALL_NAME")
verify_integrity_sha256_sidecar "$M80_INSTALL_NAME" "$M80_INSTALL_NAME.sha256" "$install_sidecar_digest"
download_integrity_asset "$M80_RELEASE_BUILD_NAME" "$build_manifest_path"
download_integrity_asset "$M80_RELEASE_BUILD_NAME.sha256" "$build_manifest_checksum_path"
build_manifest_digest=$(read_checksum_digest "$build_manifest_checksum_path" "$M80_RELEASE_BUILD_NAME")
verify_integrity_sha256_sidecar "$M80_RELEASE_BUILD_NAME" "$M80_RELEASE_BUILD_NAME.sha256" "$build_manifest_digest"
download_integrity_asset "$M80_PUBLIC_SHA256SUMS_NAME" "$public_sha256s_path"

validate_release_integrity_material prebundle "$integrity_facts_path" || fail_integrity "release integrity material verification failed"
commit_sha=
install_sha256=
# shellcheck disable=SC1090
. "$integrity_facts_path"
download_release_asset "$selected_bundle_name" "$bundle_path" "$selected_bundle_url" "yes" "integrity"
download_integrity_asset "$selected_checksum_name" "$checksum_path"
verify_integrity_sha256_sidecar "$selected_bundle_name" "$selected_checksum_name" "$selected_bundle_sha256"
actual_bundle_sha256=$(sha256sum "$bundle_path")
actual_bundle_sha256=${actual_bundle_sha256%% *}
if [ "$actual_bundle_sha256" != "$selected_bundle_sha256" ]; then
    fail_integrity "selected bundle digest mismatch: expected $selected_bundle_sha256, got $actual_bundle_sha256"
fi
actual_size=$(wc -c < "$bundle_path")
actual_size=${actual_size##* }
if [ "$actual_size" != "$selected_size_bytes" ]; then
    fail_integrity "selected bundle size mismatch: expected $selected_size_bytes, got $actual_size"
fi

validate_release_integrity_material full "$integrity_facts_path" || fail_integrity "release integrity material verification failed"
# shellcheck disable=SC1090
. "$integrity_facts_path"
echo "m80 install.sh: verified release tag=$M80_RELEASE_TAG commit=$commit_sha" >&2
echo "m80 install.sh: verified assets=$selected_bundle_name,$selected_metadata_name,$M80_INSTALL_NAME,$M80_RELEASE_BUILD_NAME,$M80_RELEASE_INTEGRITY_NAME,$selected_attestation_name" >&2
echo "m80 install.sh: install_sh_sha256=$install_sha256" >&2

mkdir "$extract_dir"
tar -xzf "$bundle_path" -C "$extract_dir" bin/m80
chmod 0755 "$extract_dir/bin/m80"
verify_extracted_m80_identity "$extract_dir/bin/m80"

run_extracted_m80_install() {
    env -i \
        HOME="${HOME:-}" \
        PATH=/usr/sbin:/usr/bin:/sbin:/bin \
        TMPDIR=/tmp \
        LANG="${LANG:-C}" \
        LC_ALL="${LC_ALL:-}" \
        LC_CTYPE="${LC_CTYPE:-}" \
        SSL_CERT_FILE="${SSL_CERT_FILE:-}" \
        SSL_CERT_DIR="${SSL_CERT_DIR:-}" \
        CURL_CA_BUNDLE="${CURL_CA_BUNDLE:-}" \
        REQUESTS_CA_BUNDLE="${REQUESTS_CA_BUNDLE:-}" \
        HTTP_PROXY="${HTTP_PROXY:-}" \
        HTTPS_PROXY="${HTTPS_PROXY:-}" \
        NO_PROXY="${NO_PROXY:-}" \
        http_proxy="${http_proxy:-}" \
        https_proxy="${https_proxy:-}" \
        no_proxy="${no_proxy:-}" \
        "$extract_dir/bin/m80" install --bundle-url "$selected_bundle_url" "$@"
}

run_extracted_m80_install "$@"
