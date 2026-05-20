#!/usr/bin/env bash
# Internal operator/test launcher for the explicit artifact-tarball override flow.

set -euo pipefail

m80_bin="${M80_BIN:-m80}"
args=()

usage() {
    cat <<'EOF'
usage: quickstart.sh --artifact-url URL [options]

Internal/operator test shim. This is not the public quickstart path.

Delegates to `m80 quickstart` for explicit local fixture or operator override
tarballs. Normal Linux first-run installs use the release install.sh:

  curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh

Artifact tarballs must match the running m80 binary. Public release tarballs are
rejected from dev builds and from mismatched release binaries.

Options:
  --artifact-url URL   Artifact tarball URL matching this m80 binary.
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
