#!/usr/bin/env bash
# End-to-end smoke test: build → preflight → launch → exec → stop.
#
# Requirements:
#   - Linux host with /dev/kvm
#   - sudo NOPASSWD (or run via sudo)
#   - Firecracker + jailer at /opt/firecracker/bin/{firecracker,jailer}
#     (override with FIRECRACKER_BIN / JAILER_BIN env vars)
#   - mkfs.ext4, e2fsck, debugfs (e2fsprogs), cp, fallocate on PATH
#   - mkfs.erofs on PATH for M80_IMAGE_KIND=minimal-erofs
#   - unsquashfs + mount/umount + truncate + curl + python3 on PATH
#   - rg on PATH for portable output matching
#   - For minimal kind: rustup target x86_64-unknown-linux-musl + /bin/busybox
#     (apt install busybox-static)
#
# Usage:
#   ./scripts/smoke.sh                              # ubuntu image, full pipeline
#   ./scripts/smoke.sh launch-only                  # skip image build
#   M80_IMAGE_KIND=minimal ./scripts/smoke.sh       # minimal image, full pipeline
#   M80_IMAGE_KIND=minimal-erofs ./scripts/smoke.sh # minimal erofs image; defaults to stripped kernel
#   M80_KERNEL_KIND=stripped ./scripts/smoke.sh     # stripped kernel smoke (m80-ci9i.6)
#   M80_PMEM_LAYERS=2 ./scripts/smoke.sh launch-only
#                                                    # generated pmem attach + DAX guest mount smoke
#   M80_PMEM_EROFS_IMAGE=/path/to/layer.erofs ./scripts/smoke.sh launch-only
#                                                    # legacy single-image pmem attach + DAX guest mount smoke
#
# Knobs (env vars):
#   M80_RUN_ROOT              (default /var/lib/m80-run; do NOT use /tmp — nodev)
#   FIRECRACKER_BIN           (default /opt/firecracker/bin/firecracker)
#   JAILER_BIN                (default /opt/firecracker/bin/jailer)
#   M80_JAIL_UID              (default = current user uid)
#   M80_JAIL_GID              (default = `kvm` group gid, falls back to user gid)
#   M80_IMAGE_KIND            (default ubuntu; "minimal" or "minimal-erofs")
#   IMAGE_BUILD_DIR           (default /tmp/m80-build/<kind>)
#   M80_KERNEL_KIND           (default stock; or "stripped")
#   M80_STRIPPED_KERNEL_PATH  (default: first glob match of
#                              crates/m80-image-build/kernels/vmlinux-m80-*.bin)
#   M80_PMEM_LAYERS           (default 0; when >0, run real-KVM pmem smoke with
#                              that many generated erofs layers)
#   M80_PMEM_EROFS_IMAGE      (optional; when set, run the pmem attach/mount
#                              real-KVM smoke instead of the default exec smoke)
#   M80_VERIFY_REFLINK_DIVERGENCE
#                              (default 0; when 1, run the rootfs overlay
#                              divergence real-KVM smoke)

set -euo pipefail

cd "$(dirname "$0")/.."

# --- knobs ---
IMAGE_KIND="${M80_IMAGE_KIND:-ubuntu}"
case "$IMAGE_KIND" in
    ubuntu|minimal|minimal-erofs) ;;
    *) echo "M80_IMAGE_KIND must be ubuntu|minimal|minimal-erofs, got: $IMAGE_KIND" >&2; exit 1 ;;
esac

if [[ "$IMAGE_KIND" == "minimal-erofs" && -z "${M80_KERNEL_KIND:-}" ]]; then
    KERNEL_KIND="stripped"
else
    KERNEL_KIND="${M80_KERNEL_KIND:-stock}"
fi
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
PMEM_LAYERS="${M80_PMEM_LAYERS:-0}"
if ! [[ "$PMEM_LAYERS" =~ ^[0-9]+$ ]]; then
    echo "M80_PMEM_LAYERS must be a non-negative integer, got: $PMEM_LAYERS" >&2
    exit 1
fi
if [[ -n "${M80_PMEM_EROFS_IMAGE:-}" && -z "${M80_PMEM_LAYERS:-}" ]]; then
    PMEM_LAYERS=1
fi

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
raise SystemExit(0 if data.get("schema_version") == 5 else 1)
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

case "$IMAGE_KIND" in
    minimal-erofs) ROOTFS_IMAGE="${IMAGE_BUILD_DIR}/output.erofs" ;;
    *)             ROOTFS_IMAGE="${IMAGE_BUILD_DIR}/output.ext4" ;;
esac

echo "=== smoke config ==="
echo "  image-kind:  $IMAGE_KIND"
echo "  kernel-kind: $KERNEL_KIND"
echo "  run-root:    $RUN_ROOT"
echo "  firecracker: $FIRECRACKER_BIN"
echo "  jailer:      $JAILER_BIN"
echo "  kernel:      $KERNEL_IMAGE"
echo "  rootfs:      $ROOTFS_IMAGE"
echo "  jail uid/gid: $JAIL_UID/$JAIL_GID"
echo "  pmem-layers: $PMEM_LAYERS"
echo

# --- build the cli ---
echo "=== build ==="
cargo build --release -p m80-cli -p m80-image-build -p m80-guestd

# For minimal kinds, additionally build a static (musl) m80-guestd.
if [[ "$IMAGE_KIND" == "minimal" || "$IMAGE_KIND" == "minimal-erofs" ]]; then
    if ! rustup target list --installed | rg -q '^x86_64-unknown-linux-musl$'; then
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
        minimal|minimal-erofs)
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
kind = "$IMAGE_KIND"

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
for optional_env in \
    M80_SKIP_CHECK_KSM \
    M80_SKIP_CHECK_SMT \
    M80_SMT_CHECK \
    M80_SKIP_CHECK_SWAP \
    M80_SKIP_CHECK_NESTED_VIRT \
    M80_SKIP_CHECK_KVM_TIMER \
    M80_SKIP_CHECK_CGROUP_FAVORDYNMODS
do
    if [[ -n "${!optional_env:-}" ]]; then
        M80_ENV+=("$optional_env=${!optional_env}")
    fi
done

# --- cleanup any prior run state ---
echo "=== cleanup prior state ==="
sudo env "${M80_ENV[@]}" ./target/release/m80 cleanup

# --- preflight ---
echo "=== preflight ==="
sudo env "${M80_ENV[@]}" ./target/release/m80 preflight

out_file="$(mktemp)"
trap 'rm -f "$out_file"' EXIT

# =========================================================================
# Reflink smoke: boot, write into rootfs overlay, verify host-side divergence
# =========================================================================
if [[ "${M80_VERIFY_REFLINK_DIVERGENCE:-0}" == "1" ]]; then
    echo "=== reflink rootfs divergence smoke ==="
    cargo test -p m80-firecracker --test reflink_rootfs_real_kvm --no-run
    reflink_test_bin="$(
        find target/debug/deps -maxdepth 1 -type f -executable \
            -name 'reflink_rootfs_real_kvm-*' | sort | tail -n 1
    )"
    if [[ -z "$reflink_test_bin" ]]; then
        echo "reflink rootfs real-KVM test binary not found" >&2
        exit 1
    fi
    if timeout 180 sudo env "${M80_ENV[@]}" \
            M80_PHASE_TRACE=1 \
            "$reflink_test_bin" --ignored --nocapture \
            reflink_rootfs_real_kvm_boot_write_diverges_overlay_from_template \
            > "$out_file" 2>&1 \
            && rg -q "REFLINK_DIVERGENCE_OK" "$out_file"; then
        echo "=== SMOKE PASSED ==="
        rg "overlay_blocks_|template_blocks_|filefrag_|REFLINK_DIVERGENCE_OK" "$out_file"
        exit 0
    fi

    echo "=== SMOKE FAILED ==="
    tail -100 "$out_file"
    exit 1
fi

# =========================================================================
# Pmem smoke: real FC /pmem PUT, guest erofs+DAX mount, then workload visibility
# =========================================================================
if [[ "$PMEM_LAYERS" -gt 0 || -n "${M80_PMEM_EROFS_IMAGE:-}" ]]; then
    echo "=== pmem real-kvm mount smoke ==="
    cargo test -p m80-firecracker --test pmem_preboot_real_kvm --no-run
    pmem_test_bin="$(
        find target/debug/deps -maxdepth 1 -type f -executable \
            -name 'pmem_preboot_real_kvm-*' | sort | tail -n 1
    )"
    if [[ -z "$pmem_test_bin" ]]; then
        echo "pmem mount test binary not found" >&2
        exit 1
    fi
    pmem_expected_layers="$PMEM_LAYERS"
    if [[ "$pmem_expected_layers" -eq 0 ]]; then
        pmem_expected_layers=1
    fi
    if timeout 120 sudo env "${M80_ENV[@]}" \
            M80_PMEM_LAYERS="$PMEM_LAYERS" \
            M80_PMEM_EROFS_IMAGE="${M80_PMEM_EROFS_IMAGE:-}" \
            M80_PHASE_TRACE=1 \
            "$pmem_test_bin" --ignored --nocapture \
            pmem_layer_real_kvm_mounts_erofs_dax_before_workload \
            > "$out_file" 2>&1 \
            && rg -q "phase_13_pmem_guest_mount" "$out_file" \
            && rg -q "pmem no-leak teardown passed" "$out_file" \
            && rg -q "pmem layer mount smoke passed: .* layers=${pmem_expected_layers}" "$out_file"; then
        for ((slot = 0; slot < pmem_expected_layers; slot++)); do
            if ! rg -q "phase_11_put_pmem_pmem_${slot}" "$out_file" \
                || ! rg -q "pmem layer digest: slot=${slot} digest=[0-9a-f]{64}" "$out_file" \
                || ! rg -q "pmem layer jail path: slot=${slot} path=.*pmem\\.${slot}\\.img" "$out_file" \
                || ! rg -q "pmem layer mount line: slot=${slot} mount_line=/dev/pmem${slot} .* erofs .*dax" "$out_file"; then
                echo "=== SMOKE FAILED ==="
                tail -80 "$out_file"
                exit 1
            fi
        done
        echo "=== SMOKE PASSED ==="
        rg "phase_11_put_pmem_pmem_[0-9]+|phase_13_pmem_guest_mount|pmem layer digest|pmem layer jail path|pmem layer mount line|pmem no-leak teardown passed|pmem layer mount smoke passed" "$out_file"
        exit 0
    fi

    echo "=== SMOKE FAILED ==="
    tail -80 "$out_file"
    exit 1
fi

# =========================================================================
# Default smoke: cold launch + exec + stop
# =========================================================================
echo "=== launch ==="
if timeout 90 sudo env "${M80_ENV[@]}" ./target/release/m80 run \
        --egress none -- /bin/echo smoke-passes \
        > "$out_file" 2>&1 \
        && rg -q "^smoke-passes$" "$out_file"; then
    echo "=== SMOKE PASSED ==="
    rg "^smoke-passes$|exit_code=0|Firecracker exiting" "$out_file"
    exit 0
fi

echo "=== SMOKE FAILED ==="
tail -40 "$out_file"
exit 1
