#!/usr/bin/env bash
# Cold-cold snapshot restore baseline for m80-jp6ik.43.
#
# Builds the real-KVM restore bench target as the current user, then runs the
# bench binary under sudo so Firecracker, jailer, mounts, and drop_caches use
# the same privileged substrate as the smoke tests.

set -euo pipefail

cd "$(dirname "$0")/.."

N="${N:-50}"
FILE_READ_SAMPLES="${FILE_READ_SAMPLES:-10}"
KIND="${KIND:-minimal}"
KERNEL_KIND="${KERNEL_KIND:-${M80_KERNEL_KIND:-stripped}}"
VCPU_COUNT="${VCPU_COUNT:-}"
MEM_SIZE_MIB="${MEM_SIZE_MIB:-}"
IMAGE_DIR="${IMAGE_BUILD_DIR:-${IMAGE_BUILD_DIR_MINIMAL:-/tmp/m80-build/minimal}}"
OUTDIR="crates/m80-firecracker/benches"
SNAPSHOT_OUT="${SNAPSHOT_OUT:-$OUTDIR/snapshots/cold-restore-N${N}.json}"
DRY_RUN=0

usage() {
    cat <<'EOF'
bench-restore-cold.sh - m80-jp6ik.43 cold-cold restore baseline

Usage:
  scripts/bench-restore-cold.sh [--dry-run]

Env vars:
  N=50
  FILE_READ_SAMPLES=10
  KIND=minimal
  KERNEL_KIND=stripped
  VCPU_COUNT=1
  MEM_SIZE_MIB=1024
  IMAGE_BUILD_DIR=/tmp/m80-build/minimal
  IMAGE_BUILD_DIR_MINIMAL=/tmp/m80-build/minimal
  SNAPSHOT_OUT=crates/m80-firecracker/benches/snapshots/cold-restore-N50.json

Output:
  $SNAPSHOT_OUT
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run) DRY_RUN=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) echo "unknown flag: $1" >&2; usage >&2; exit 2 ;;
    esac
done

case "$KIND" in
    minimal) ;;
    *) echo "KIND must be minimal for snapshot restore baseline" >&2; exit 1 ;;
esac

plan() {
    echo "=== bench-restore-cold plan ==="
    echo "  N=$N  FILE_READ_SAMPLES=$FILE_READ_SAMPLES"
    echo "  KIND=$KIND  KERNEL_KIND=$KERNEL_KIND"
    [[ -n "$VCPU_COUNT" ]] && echo "  VCPU_COUNT=$VCPU_COUNT"
    [[ -n "$MEM_SIZE_MIB" ]] && echo "  MEM_SIZE_MIB=$MEM_SIZE_MIB"
    echo "  image_dir=$IMAGE_DIR"
    echo "  output: $SNAPSHOT_OUT"
}

plan
if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "(dry-run) skipping build and launches."
    exit 0
fi

mkdir -p "$(dirname "$SNAPSHOT_OUT")"
cargo build --release -p m80-jailer-harden
cargo build --release -p m80-firecracker --bench snapshot_restore_latency

bench_bin=""
for candidate in target/release/deps/snapshot_restore_latency-*; do
    [[ -e "$candidate" ]] || continue
    if [[ -f "$candidate" && -x "$candidate" && ( -z "$bench_bin" || "$candidate" -nt "$bench_bin" ) ]]; then
        bench_bin="$candidate"
    fi
done
if [[ -z "$bench_bin" ]]; then
    echo "snapshot_restore_latency bench binary not found" >&2
    exit 1
fi

BENCH_ENV=()
[[ -n "$VCPU_COUNT" ]] && BENCH_ENV+=(M80_SNAPSHOT_BENCH_VCPU_COUNT="$VCPU_COUNT")
[[ -n "$MEM_SIZE_MIB" ]] && BENCH_ENV+=(M80_SNAPSHOT_BENCH_MEM_SIZE_MIB="$MEM_SIZE_MIB")

sudo env \
    N="$N" \
    M80_RESTORE_FILE_READ_SAMPLES="$FILE_READ_SAMPLES" \
    M80_SNAPSHOT_BENCH_OUTPUT="$SNAPSHOT_OUT" \
    "${BENCH_ENV[@]}" \
    IMAGE_BUILD_DIR="$IMAGE_DIR" \
    M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
    M80_JAILER_BIN=/opt/firecracker/bin/jailer \
    M80_JAILER_HARDEN_BIN="${M80_JAILER_HARDEN_BIN:-$PWD/target/release/m80-jailer-harden}" \
    M80_KERNEL_IMAGE="$IMAGE_DIR/vmlinux" \
    M80_KERNEL_KIND="$KERNEL_KIND" \
    M80_ROOTFS_IMAGE="$IMAGE_DIR/output.ext4" \
    M80_RUN_ROOT=/var/lib/m80-run \
    M80_FIRECRACKER_VERSION=v1.15.1 \
    M80_JAIL_UID="$(id -u)" \
    M80_JAIL_GID="$(getent group kvm | cut -d: -f3 || id -g)" \
    "$bench_bin"

echo "restore snapshot: $SNAPSHOT_OUT"
