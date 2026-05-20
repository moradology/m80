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
M80_IMAGE_KIND='minimal'
M80_SELECTOR_COLUMNS='os	arch	image_kind	bundle_name	bundle_url	bundle_sha256	size_bytes	metadata_name	metadata_sha256	checksum_name	signature_name	attestation_name	m80_version'

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

download_asset() {
    asset_name=$1
    dest=$2
    url=$(asset_url "$asset_name")
    curl -fsSL "$url" -o "$dest" || fail "failed to download $asset_name from $url"
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
    asset_name=$1
    checksum_name=$2
    expected_digest=$3
    checksum_path="$tmp/$checksum_name"
    observed_digest=$(read_checksum_digest "$checksum_path" "$asset_name")
    if [ "$observed_digest" != "$expected_digest" ]; then
        fail "checksum digest mismatch for $asset_name: expected $expected_digest from selector, got $observed_digest from $checksum_name"
    fi
    (cd "$tmp" && sha256sum -c "$checksum_name" >/dev/null) || fail "checksum verification failed for $asset_name"
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
    selected_checksum_name=

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
            selected_checksum_name=${11}
        fi
    done < "$selector_path"

    [ "$line_no" -ge 3 ] || fail "bootstrap selector missing header rows from $(asset_url "$M80_BOOTSTRAP_SELECTOR_NAME")"
    [ "$match_count" -eq 1 ] || fail "bootstrap selector missing tuple for release_tag=${M80_RELEASE_TAG} os=${requested_os} arch=${requested_arch} image_kind=${requested_image_kind} selector=$(asset_url "$M80_BOOTSTRAP_SELECTOR_NAME") index=$(asset_url "$M80_ASSET_INDEX_NAME")"

    validate_sha256 "bootstrap selector bundle_sha256" "$selected_bundle_sha256"
    validate_positive_int "bootstrap selector size_bytes" "$selected_size_bytes"
    case "$selected_bundle_name" in
        */*) fail "bootstrap selector bundle_name must not contain slash" ;;
    esac
    case "$selected_checksum_name" in
        */*) fail "bootstrap selector checksum_name must not contain slash" ;;
    esac
    expected_bundle_url=$(asset_url "$selected_bundle_name")
    if [ "$selected_bundle_url" != "$expected_bundle_url" ]; then
        fail "bootstrap selector bundle_url mismatch for ${requested_os}/${requested_arch}/${requested_image_kind}: expected $expected_bundle_url, got $selected_bundle_url"
    fi
    if [ "$selected_checksum_name" != "$selected_bundle_name.sha256" ]; then
        fail "bootstrap selector checksum_name mismatch for $selected_bundle_name"
    fi
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

echo "m80 install.sh: bundle=$selected_bundle_url" >&2

curl -fsSL "$selected_bundle_url" -o "$bundle_path" || fail "failed to download selected bundle $selected_bundle_name from $selected_bundle_url"
download_asset "$selected_checksum_name" "$checksum_path"
verify_sha256_sidecar "$selected_bundle_name" "$selected_checksum_name" "$selected_bundle_sha256"
actual_bundle_sha256=$(sha256sum "$bundle_path")
actual_bundle_sha256=${actual_bundle_sha256%% *}
if [ "$actual_bundle_sha256" != "$selected_bundle_sha256" ]; then
    fail "selected bundle digest mismatch: expected $selected_bundle_sha256, got $actual_bundle_sha256"
fi
actual_size=$(wc -c < "$bundle_path")
actual_size=${actual_size##* }
if [ "$actual_size" != "$selected_size_bytes" ]; then
    fail "selected bundle size mismatch: expected $selected_size_bytes, got $actual_size"
fi

mkdir "$extract_dir"
tar -xzf "$bundle_path" -C "$extract_dir" bin/m80
chmod 0755 "$extract_dir/bin/m80"

"$extract_dir/bin/m80" install --bundle-url "file://$bundle_path" "$@"
