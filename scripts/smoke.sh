#!/usr/bin/env bash
# End-to-end smoke test: build → preflight → launch → exec → stop.
#
# Requirements:
#   - Linux host with /dev/kvm
#   - sudo NOPASSWD (or run via sudo)
#   - Firecracker + jailer at /opt/firecracker/bin/{firecracker,jailer}
#     (override with FIRECRACKER_BIN / JAILER_BIN env vars)
#   - mkfs.ext4, e2fsck, debugfs (e2fsprogs) on PATH
#   - unsquashfs + mount/umount + truncate + curl on PATH
#
# Usage:
#   ./scripts/smoke.sh                 # full pipeline (build image, preflight, launch)
#   ./scripts/smoke.sh launch-only     # skip image build (assumes /tmp/m80-build/output already populated)
#
# Knobs (env vars):
#   M80_RUN_ROOT       (default /var/lib/m80-run; do NOT use /tmp — nodev)
#   FIRECRACKER_BIN    (default /opt/firecracker/bin/firecracker)
#   JAILER_BIN         (default /opt/firecracker/bin/jailer)
#   M80_JAIL_UID       (default = current user uid)
#   M80_JAIL_GID       (default = `kvm` group gid, falls back to user gid)
#   IMAGE_BUILD_DIR    (default /tmp/m80-build/output)

set -euo pipefail

cd "$(dirname "$0")/.."

# --- knobs ---
RUN_ROOT="${M80_RUN_ROOT:-/var/lib/m80-run}"
FIRECRACKER_BIN="${FIRECRACKER_BIN:-/opt/firecracker/bin/firecracker}"
JAILER_BIN="${JAILER_BIN:-/opt/firecracker/bin/jailer}"
IMAGE_BUILD_DIR="${IMAGE_BUILD_DIR:-/tmp/m80-build/output}"
JAIL_UID="${M80_JAIL_UID:-$(id -u)}"
JAIL_GID="${M80_JAIL_GID:-$(getent group kvm | cut -d: -f3 || id -g)}"

KERNEL_IMAGE="${IMAGE_BUILD_DIR}/vmlinux"
ROOTFS_IMAGE="${IMAGE_BUILD_DIR}/output.ext4"
FIRECRACKER_VERSION="${M80_FIRECRACKER_VERSION:-v1.15.1}"

mode="${1:-full}"

echo "=== smoke config ==="
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

# --- build the guest image (full mode only) ---
if [[ "$mode" == "full" ]]; then
    if [[ ! -f "$ROOTFS_IMAGE" || ! -f "${ROOTFS_IMAGE}.manifest.json" ]]; then
        echo "=== build guest image ==="
        mkdir -p "$IMAGE_BUILD_DIR"
        cat > /tmp/m80-image-build.toml <<EOF
[kernel]
version = "v1.15"
arch = "x86_64"

[rootfs]
size = "1GiB"

[guestd]
binary = "$(pwd)/target/release/m80-guestd"

[output]
dir = "$IMAGE_BUILD_DIR"
EOF
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

# --- launch + echo + stop ---
out_file="$(mktemp)"
trap 'rm -f "$out_file"' EXIT
echo "=== launch ==="
if timeout 90 sudo env "${M80_ENV[@]}" ./target/release/m80 launch \
        --network noegress -- /bin/echo smoke-passes \
        > "$out_file" 2>&1 \
        && grep -q "^smoke-passes$" "$out_file"; then
    echo "=== SMOKE PASSED ==="
    grep -E "^smoke-passes$|exit_code=0|Firecracker exiting" "$out_file"
    exit 0
fi

echo "=== SMOKE FAILED ==="
tail -40 "$out_file"
exit 1
