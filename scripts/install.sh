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
need mktemp
need chmod
need mkdir
need rm
need uname
need wc

preflight_attestation_verifier() {
    if ! gh_version="$(gh --version 2>&1)"; then
        echo "m80 install.sh: release attestation verifier missing: gh" >&2
        echo "m80 install.sh: signed m80 release installs require gh attestation verify before downloading release assets" >&2
        echo "m80 install.sh: observed version output: $gh_version" >&2
        echo "m80 install.sh: Install or upgrade GitHub CLI with attestation support on Linux: https://cli.github.com/packages" >&2
        exit 127
    fi
    if ! gh_help="$(gh attestation verify --help 2>&1)"; then
        echo "m80 install.sh: release attestation verifier unsupported: gh attestation verify --help failed" >&2
        echo "m80 install.sh: signed m80 release installs require gh attestation verify before downloading release assets" >&2
        echo "m80 install.sh: observed version output: $gh_version" >&2
        echo "m80 install.sh: observed help output: $gh_help" >&2
        echo "m80 install.sh: Install or upgrade GitHub CLI with attestation support on Linux: https://cli.github.com/packages" >&2
        exit 1
    fi
    for flag in --repo --bundle --signer-workflow --cert-oidc-issuer --source-ref --source-digest --deny-self-hosted-runners --format; do
        case "$gh_help" in
            *"$flag"*) ;;
            *)
                echo "m80 install.sh: release attestation verifier unsupported: gh attestation verify --help is missing $flag" >&2
                echo "m80 install.sh: observed version output: $gh_version" >&2
                echo "m80 install.sh: observed help output: $gh_help" >&2
                echo "m80 install.sh: Install or upgrade GitHub CLI with attestation support on Linux: https://cli.github.com/packages" >&2
                exit 1
                ;;
        esac
    done
}

preflight_attestation_verifier

need python3

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

asset_url() {
    printf '%s/%s\n' "$M80_RELEASE_BASE_URL" "$1"
}

integrity_retry_command() {
    printf 'curl -fsSL %s | sudo sh\n' "$(asset_url "$M80_INSTALL_NAME")"
}

fail_integrity() {
    fail "$*; retry pinned command: $(integrity_retry_command)"
}

download_asset() {
    asset_name=$1
    dest=$2
    url=$(asset_url "$asset_name")
    curl -fsSL "$url" -o "$dest" || fail "failed to download $asset_name from $url"
}

download_integrity_asset() {
    asset_name=$1
    dest=$2
    url=$(asset_url "$asset_name")
    curl -fsSL "$url" -o "$dest" || fail_integrity "failed to download $asset_name from $url"
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
    [ "$match_count" -eq 1 ] || fail "bootstrap selector missing tuple for release_tag=${M80_RELEASE_TAG} os=${requested_os} arch=${requested_arch} image_kind=${requested_image_kind} selector=$(asset_url "$M80_BOOTSTRAP_SELECTOR_NAME") index=$(asset_url "$M80_ASSET_INDEX_NAME")"

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
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
MECHANISM = "github-artifact-attestation"
INTEGRITY_NAME = "m80-release-integrity.json"
ATTESTATION_METADATA_NAME = "m80-release-attestation.json"
INSTALL_NAME = "install.sh"
ASSET_INDEX_NAME = "m80-release-assets.json"
BOOTSTRAP_SELECTOR_NAME = "m80-bootstrap-selector.tsv"
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


def verify_asset_index(metadata: dict) -> None:
    index = read_json(tmp / ASSET_INDEX_NAME, "asset index")
    require_exact_fields(index, EXPECTED_INDEX_FIELDS, "asset index")
    require(index["schema_version"] == 1, "release integrity unsupported asset index schema_version")
    require(index["release_tag"] == release_tag, "release integrity asset index release_tag mismatch")
    require(isinstance(index["assets"], list) and index["assets"], "release integrity asset index assets must not be empty")
    selected = None
    for asset in index["assets"]:
        require(isinstance(asset, dict), "release integrity asset index asset must be an object")
        name = asset.get("name") if isinstance(asset.get("name"), str) else "<unknown>"
        require_exact_fields(asset, EXPECTED_ASSET_FIELDS, f"asset index asset {name}")
        if (asset["os"], asset["arch"], asset["image_kind"]) == (host_os, host_arch, image_kind):
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
        require(selected[field] == expected_value, f"release integrity asset index {field} mismatch for {bundle_name}")


def verify_subjects(material: dict) -> str:
    subjects = material["subjects"]
    require(isinstance(subjects, list), "release integrity subjects must be a list")
    expected = {
        bundle_name: "release-bundle",
        checksum_name: "checksum-sidecar",
        INSTALL_NAME: "installer",
        f"{INSTALL_NAME}.sha256": "checksum-sidecar",
        metadata_name: "bundle-metadata",
        f"{metadata_name}.sha256": "checksum-sidecar",
        ASSET_INDEX_NAME: "asset-index",
        f"{ASSET_INDEX_NAME}.sha256": "checksum-sidecar",
        BOOTSTRAP_SELECTOR_NAME: "bootstrap-selector",
        f"{BOOTSTRAP_SELECTOR_NAME}.sha256": "checksum-sidecar",
        PUBLIC_SHA256SUMS_NAME: "checksum-manifest",
    }
    skipped = {bundle_name, checksum_name} if phase == "prebundle" else set()
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
    missing = sorted(set(expected) - set(by_name) - skipped)
    require(not missing, "release integrity missing subject(s): " + ", ".join(missing))
    extra = sorted(set(by_name) - set(expected))
    require(not extra, "release integrity unexpected subject(s): " + ", ".join(extra))
    for name, subject in by_name.items():
        if name in skipped:
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
metadata = verify_metadata(material)
verify_asset_index(metadata)
install_sha256 = verify_subjects(material)
print(f"commit_sha={commit_sha}")
print(f"install_sha256={install_sha256}")
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
download_integrity_asset "$M80_PUBLIC_SHA256SUMS_NAME" "$public_sha256s_path"

validate_release_integrity_material prebundle "$integrity_facts_path" || fail_integrity "release integrity material verification failed"
commit_sha=
install_sha256=
# shellcheck disable=SC1090
. "$integrity_facts_path"
gh attestation verify "$integrity_path" \
    --repo "${M80_PUBLIC_RELEASE_OWNER}/${M80_PUBLIC_RELEASE_REPO}" \
    --bundle "$attestation_bundle_path" \
    --signer-workflow "$M80_TRUST_SIGNER_WORKFLOW" \
    --cert-oidc-issuer "$M80_TRUST_SIGNER_ISSUER" \
    --source-ref "refs/tags/${M80_RELEASE_TAG}" \
    --source-digest "$commit_sha" \
    --deny-self-hosted-runners \
    --format json >/dev/null || fail_integrity "release attestation verification failed for $M80_RELEASE_INTEGRITY_NAME"

curl -fsSL "$selected_bundle_url" -o "$bundle_path" || fail_integrity "failed to download selected bundle $selected_bundle_name from $selected_bundle_url"
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
echo "m80 install.sh: verified assets=$selected_bundle_name,$selected_metadata_name,$M80_INSTALL_NAME,$M80_RELEASE_INTEGRITY_NAME,$selected_attestation_name" >&2
echo "m80 install.sh: install_sh_sha256=$install_sha256" >&2

mkdir "$extract_dir"
tar -xzf "$bundle_path" -C "$extract_dir" bin/m80
chmod 0755 "$extract_dir/bin/m80"

"$extract_dir/bin/m80" install --bundle-url "file://$bundle_path" "$@"
