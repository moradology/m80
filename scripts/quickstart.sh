#!/usr/bin/env bash
# Thin no-checkout launcher for the real CLI quickstart flow.

set -euo pipefail

m80_bin="${M80_BIN:-m80}"
args=()

usage() {
    cat <<'EOF'
usage: quickstart.sh --artifact-url URL [options]

Delegates to `m80 quickstart`, which downloads release artifacts, verifies
checksums, installs them, then runs the smallest process-wrapper probe unless
`--no-run` is set.

Options:
  --artifact-url URL   Release artifact tarball URL.
  --artifact-dir PATH  Artifact install dir.
  --run-root PATH      m80 run-root dir.
  --m80-bin PATH       m80 binary to execute (default: M80_BIN or m80 on PATH).
  --no-run             Install and verify artifacts but do not run echo.
  -h, --help           Print this help.
EOF
}

while (($# > 0)); do
    case "$1" in
        --m80-bin)
            m80_bin="${2:?--m80-bin requires a value}"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            args+=("$1")
            shift
            ;;
    esac
done

exec "$m80_bin" quickstart "${args[@]}"
