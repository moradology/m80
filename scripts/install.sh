#!/bin/sh
# Versioned release installer template. package-release-bundle.py renders the
# placeholders below for the concrete GitHub release asset.

set -eu

M80_RELEASE_TAG='@M80_RELEASE_TAG@'
M80_BUNDLE_URL='@M80_BUNDLE_URL@'
M80_BUNDLE_NAME='@M80_BUNDLE_NAME@'

usage() {
    cat <<EOF
usage: install.sh [m80 install options]

Downloads the m80 ${M80_RELEASE_TAG} release bundle, verifies its checksum,
then runs the bundled versioned m80 installer.

Common options passed through to m80 install:
  --install-root PATH  Install root, default /opt/m80
  --dry-run            Print the install plan without writing host state
  -h, --help           Print this help
EOF
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

tmp="$(mktemp -d "${TMPDIR:-/tmp}/m80-install.XXXXXX")"
cleanup() {
    rm -rf "$tmp"
}
trap cleanup EXIT HUP INT TERM

bundle_path="$tmp/$M80_BUNDLE_NAME"
checksum_path="$tmp/$M80_BUNDLE_NAME.sha256"
extract_dir="$tmp/extract"

echo "m80 install.sh: release=$M80_RELEASE_TAG" >&2
echo "m80 install.sh: bundle=$M80_BUNDLE_URL" >&2

curl -fsSL "$M80_BUNDLE_URL" -o "$bundle_path"
curl -fsSL "$M80_BUNDLE_URL.sha256" -o "$checksum_path"
(cd "$tmp" && sha256sum -c "$M80_BUNDLE_NAME.sha256" >/dev/null)

mkdir "$extract_dir"
tar -xzf "$bundle_path" -C "$extract_dir" bin/m80
chmod 0755 "$extract_dir/bin/m80"

"$extract_dir/bin/m80" install --bundle-url "file://$bundle_path" "$@"
