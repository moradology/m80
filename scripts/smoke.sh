#!/usr/bin/env bash
# End-to-end smoke test: build → preflight → launch → exec → stop.
#
# Requirements:
#   - Linux host with /dev/kvm
#   - sudo NOPASSWD (or run via sudo)
#   - Firecracker + jailer at /opt/firecracker/bin/{firecracker,jailer}
#     (override with FIRECRACKER_BIN / JAILER_BIN env vars)
#   - mkfs.ext4, e2fsck, debugfs (e2fsprogs), cp, fallocate on PATH
#   - unsquashfs + mount/umount + truncate + curl + python3 on PATH
#   - For minimal kind: rustup target x86_64-unknown-linux-musl + /bin/busybox
#     (apt install busybox-static)
#
# Usage:
#   ./scripts/smoke.sh                              # ubuntu image, full pipeline
#   ./scripts/smoke.sh launch-only                  # skip image build
#   M80_IMAGE_KIND=minimal ./scripts/smoke.sh       # minimal image, full pipeline
#   M80_KERNEL_KIND=stripped ./scripts/smoke.sh     # stripped kernel smoke (m80-ci9i.6)
#
# Knobs (env vars):
#   M80_RUN_ROOT              (default /var/lib/m80-run; do NOT use /tmp — nodev)
#   FIRECRACKER_BIN           (default /opt/firecracker/bin/firecracker)
#   JAILER_BIN                (default /opt/firecracker/bin/jailer)
#   M80_JAIL_UID              (default = current user uid)
#   M80_JAIL_GID              (default = `kvm` group gid, falls back to user gid)
#   M80_IMAGE_KIND            (default ubuntu; or "minimal")
#   IMAGE_BUILD_DIR           (default /tmp/m80-build/<kind>)
#   M80_KERNEL_KIND           (default stock; or "stripped")
#   M80_STRIPPED_KERNEL_PATH  (default: first glob match of
#                              crates/m80-image-build/kernels/vmlinux-m80-*.bin)

set -euo pipefail

cd "$(dirname "$0")/.."

# --- knobs ---
IMAGE_KIND="${M80_IMAGE_KIND:-ubuntu}"
case "$IMAGE_KIND" in
    ubuntu|minimal) ;;
    *) echo "M80_IMAGE_KIND must be ubuntu|minimal, got: $IMAGE_KIND" >&2; exit 1 ;;
esac

KERNEL_KIND="${M80_KERNEL_KIND:-stock}"
case "$KERNEL_KIND" in
    stock|stripped) ;;
    *) echo "M80_KERNEL_KIND must be stock|stripped, got: $KERNEL_KIND" >&2; exit 1 ;;
esac

RUN_ROOT="${M80_RUN_ROOT:-/var/lib/m80-run}"
FIRECRACKER_BIN="${FIRECRACKER_BIN:-/opt/firecracker/bin/firecracker}"
JAILER_BIN="${JAILER_BIN:-/opt/firecracker/bin/jailer}"
IMAGE_BUILD_DIR="${IMAGE_BUILD_DIR:-/tmp/m80-build/$IMAGE_KIND}"
JAIL_UID="${M80_JAIL_UID:-$(id -u)}"
JAIL_GID="${M80_JAIL_GID:-$(getent group kvm | cut -d: -f3 || id -g)}"

FIRECRACKER_VERSION="${M80_FIRECRACKER_VERSION:-v1.15.1}"

mode="${1:-full}"

manifest_schema_ok() {
    local manifest_path="$1"
    python3 - "$manifest_path" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
if not path.exists():
    raise SystemExit(1)
try:
    data = json.loads(path.read_text())
except Exception:
    raise SystemExit(1)
raise SystemExit(0 if data.get("schema_version") == 4 else 1)
PY
}

# --- resolve kernel image ---
# For stripped kernel kind, find the built artifact (requires prior
# Docker build of the m80-image-build kernel pipeline; see
# crates/m80-image-build/kernel-builder/ and m80-ci9i.2).
if [[ "$KERNEL_KIND" == "stripped" ]]; then
    if [[ -n "${M80_STRIPPED_KERNEL_PATH:-}" ]]; then
        KERNEL_IMAGE="$M80_STRIPPED_KERNEL_PATH"
    else
        # Use the newest built stripped kernel artifact.
        shopt -s nullglob
        stripped_hits=(crates/m80-image-build/kernels/vmlinux-m80-*.bin)
        shopt -u nullglob
        if [[ "${#stripped_hits[@]}" -eq 0 || ! -f "${stripped_hits[0]}" ]]; then
            echo "# stripped kernel not yet built; skipping (run m80-image-build kernel build first)"
            echo "# to build: see crates/m80-image-build/kernel-builder/Dockerfile (m80-ci9i.2)"
            exit 0
        fi
        KERNEL_IMAGE="${stripped_hits[0]}"
        for candidate in "${stripped_hits[@]}"; do
            if [[ "$candidate" -nt "$KERNEL_IMAGE" ]]; then
                KERNEL_IMAGE="$candidate"
            fi
        done
    fi
    echo "# using stripped kernel: $KERNEL_IMAGE"
else
    KERNEL_IMAGE="${IMAGE_BUILD_DIR}/vmlinux"
fi

ROOTFS_IMAGE="${IMAGE_BUILD_DIR}/output.ext4"

echo "=== smoke config ==="
echo "  image-kind:  $IMAGE_KIND"
echo "  kernel-kind: $KERNEL_KIND"
echo "  run-root:    $RUN_ROOT"
echo "  firecracker: $FIRECRACKER_BIN"
echo "  jailer:      $JAILER_BIN"
echo "  kernel:      $KERNEL_IMAGE"
echo "  rootfs:      $ROOTFS_IMAGE"
echo "  jail uid/gid: $JAIL_UID/$JAIL_GID"
echo

# --- build the cli ---
echo "=== build ==="
cargo build --release -p m80-cli -p m80-image-build -p m80-guestd

# For minimal kind, additionally build a static (musl) m80-guestd.
if [[ "$IMAGE_KIND" == "minimal" ]]; then
    if ! rustup target list --installed | grep -q '^x86_64-unknown-linux-musl$'; then
        echo "=== adding rustup target x86_64-unknown-linux-musl ==="
        rustup target add x86_64-unknown-linux-musl
    fi
    echo "=== build static m80-guestd (musl) ==="
    cargo build --release --target x86_64-unknown-linux-musl -p m80-guestd
fi

# --- build the guest image (full mode only) ---
if [[ "$mode" == "full" ]]; then
    if [[ ! -f "$ROOTFS_IMAGE" || ! -f "${ROOTFS_IMAGE}.manifest.json" ]] \
        || ! manifest_schema_ok "${ROOTFS_IMAGE}.manifest.json"; then
        echo "=== build guest image ($IMAGE_KIND) ==="
        mkdir -p "$IMAGE_BUILD_DIR"
        case "$IMAGE_KIND" in
        ubuntu)
            guestd_bin="$(pwd)/target/release/m80-guestd"
            cat > /tmp/m80-image-build.toml <<EOF
[kernel]
version = "$FIRECRACKER_VERSION"
artifact_track = "v1.15"
arch = "x86_64"

[rootfs]
size = "1GiB"

[guestd]
binary = "$guestd_bin"

[output]
dir = "$IMAGE_BUILD_DIR"
EOF
            ;;
        minimal)
            guestd_bin="$(pwd)/target/x86_64-unknown-linux-musl/release/m80-guestd"
            if [[ ! -x "$guestd_bin" ]]; then
                echo "missing static guestd at $guestd_bin" >&2
                exit 1
            fi
            if [[ ! -x /bin/busybox ]]; then
                echo "missing /bin/busybox; install busybox-static" >&2
                exit 1
            fi
            cat > /tmp/m80-image-build.toml <<EOF
[kernel]
version = "$FIRECRACKER_VERSION"
artifact_track = "v1.15"
arch = "x86_64"

[rootfs]
size = "256MiB"
kind = "minimal"

[guestd]
binary = "$guestd_bin"

[output]
dir = "$IMAGE_BUILD_DIR"
EOF
            ;;
        esac
        sudo ./target/release/m80-image-build run --config /tmp/m80-image-build.toml
    else
        echo "=== guest image already built at $IMAGE_BUILD_DIR ==="
    fi
fi

# --- ensure run-root exists and is writable by our uid ---
sudo mkdir -p "$RUN_ROOT"
sudo chown "$(id -u):$(id -g)" "$RUN_ROOT"

# --- common env block ---
M80_ENV=(
    M80_FIRECRACKER_BIN="$FIRECRACKER_BIN"
    M80_JAILER_BIN="$JAILER_BIN"
    M80_KERNEL_IMAGE="$KERNEL_IMAGE"
    M80_ROOTFS_IMAGE="$ROOTFS_IMAGE"
    M80_KERNEL_KIND="$KERNEL_KIND"
    M80_RUN_ROOT="$RUN_ROOT"
    M80_FIRECRACKER_VERSION="$FIRECRACKER_VERSION"
    M80_JAIL_UID="$JAIL_UID"
    M80_JAIL_GID="$JAIL_GID"
)

# --- cleanup any prior run state ---
echo "=== cleanup prior state ==="
sudo env "${M80_ENV[@]}" ./target/release/m80 cleanup

# --- preflight ---
echo "=== preflight ==="
sudo env "${M80_ENV[@]}" ./target/release/m80 preflight

out_file="$(mktemp)"
trap 'rm -f "$out_file"' EXIT

# =========================================================================
# Default smoke: cold launch + exec + stop
# =========================================================================
echo "=== launch ==="
if timeout 90 sudo env "${M80_ENV[@]}" ./target/release/m80 run \
        --egress none -- /bin/echo smoke-passes \
        > "$out_file" 2>&1 \
        && grep -q "^smoke-passes$" "$out_file"; then
    echo "=== SMOKE PASSED ==="
    grep -E "^smoke-passes$|exit_code=0|Firecracker exiting" "$out_file"
    exit 0
fi

echo "=== SMOKE FAILED ==="
tail -40 "$out_file"
exit 1
