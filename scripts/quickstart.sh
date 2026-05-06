#!/usr/bin/env sh
# Download m80 release artifacts, verify their checksums, install them into the
# default artifact directory, then run the smallest constrained-process probe.

set -eu

artifact_url=""
artifact_dir="${M80_ARTIFACT_DIR:-/opt/m80/artifacts}"
run_root="${M80_RUN_ROOT:-/var/run/m80}"
m80_bin="${M80_BIN:-m80}"
run_probe=1

usage() {
    cat <<'EOF'
usage: quickstart.sh --artifact-url URL [options]

Options:
  --artifact-url URL   Release artifact tarball URL.
  --artifact-dir PATH  Artifact install dir (default: /opt/m80/artifacts).
  --run-root PATH      m80 run-root dir (default: /var/run/m80).
  --m80-bin PATH       m80 binary to execute (default: m80 on PATH).
  --no-run            Install and verify artifacts but do not run echo.
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --artifact-url)
            artifact_url="${2:?--artifact-url requires a value}"
            shift 2
            ;;
        --artifact-dir)
            artifact_dir="${2:?--artifact-dir requires a value}"
            shift 2
            ;;
        --run-root)
            run_root="${2:?--run-root requires a value}"
            shift 2
            ;;
        --m80-bin)
            m80_bin="${2:?--m80-bin requires a value}"
            shift 2
            ;;
        --no-run)
            run_probe=0
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [ -z "$artifact_url" ]; then
    echo "missing required --artifact-url" >&2
    usage >&2
    exit 2
fi

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "missing required command: $1" >&2
        exit 2
    fi
}

need_cmd curl
need_cmd tar
need_cmd sha256sum

tmp="${TMPDIR:-/tmp}/m80-quickstart.$$"
mkdir -p "$tmp"
cleanup() {
    rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

tarball="$tmp/artifacts.tar.gz"
extract_dir="$tmp/artifacts"
mkdir -p "$extract_dir"

echo "downloading m80 artifacts: $artifact_url" >&2
curl -fsSL "$artifact_url" -o "$tarball"
tar -xzf "$tarball" -C "$extract_dir"

if [ ! -f "$extract_dir/SHA256SUMS" ]; then
    echo "artifact tarball is missing SHA256SUMS" >&2
    exit 1
fi

(cd "$extract_dir" && sha256sum -c SHA256SUMS)

for file in vmlinux output.ext4 output.ext4.manifest.json m80-guestd; do
    if [ ! -f "$extract_dir/$file" ]; then
        echo "artifact tarball is missing $file" >&2
        exit 1
    fi
done

install -d -m 0755 "$artifact_dir"
install -d -m 0755 "$run_root"
cp "$extract_dir/vmlinux" "$artifact_dir/vmlinux"
cp "$extract_dir/output.ext4" "$artifact_dir/output.ext4"
cp "$extract_dir/output.ext4.manifest.json" "$artifact_dir/output.ext4.manifest.json"
cp "$extract_dir/m80-guestd" "$artifact_dir/m80-guestd"

echo "installed artifacts under $artifact_dir" >&2

if [ "$run_probe" = 1 ]; then
    M80_KERNEL_IMAGE="$artifact_dir/vmlinux" \
        M80_ROOTFS_IMAGE="$artifact_dir/output.ext4" \
        M80_RUN_ROOT="$run_root" \
        "$m80_bin" run -- echo hello
fi

cat >&2 <<EOF

Next:
  M80_KERNEL_IMAGE=$artifact_dir/vmlinux M80_ROOTFS_IMAGE=$artifact_dir/output.ext4 M80_RUN_ROOT=$run_root $m80_bin run --workspace . --cwd /workspace -- ls
  M80_KERNEL_IMAGE=$artifact_dir/vmlinux M80_ROOTFS_IMAGE=$artifact_dir/output.ext4 M80_RUN_ROOT=$run_root $m80_bin run --egress none -- echo isolated
  M80_KERNEL_IMAGE=$artifact_dir/vmlinux M80_ROOTFS_IMAGE=$artifact_dir/output.ext4 M80_RUN_ROOT=$run_root $m80_bin run -it --workspace . -- sh
EOF
