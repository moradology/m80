#!/usr/bin/env bash
# End-to-end smoke test: build → preflight → launch → exec → stop.
#
# Requirements:
#   - Linux host with /dev/kvm
#   - sudo NOPASSWD (or run via sudo)
#   - Firecracker + jailer at /opt/firecracker/bin/{firecracker,jailer}
#     (override with FIRECRACKER_BIN / JAILER_BIN env vars)
#   - mkfs.ext4, e2fsck, debugfs (e2fsprogs), cp, fallocate on PATH
#   - unsquashfs + mount/umount + truncate + curl on PATH
#   - For minimal kind: rustup target x86_64-unknown-linux-musl + /bin/busybox
#     (apt install busybox-static)
#
# Usage:
#   ./scripts/smoke.sh                              # ubuntu image, full pipeline
#   ./scripts/smoke.sh launch-only                  # skip image build
#   M80_IMAGE_KIND=minimal ./scripts/smoke.sh       # minimal image, full pipeline
#   M80_KERNEL_KIND=stripped ./scripts/smoke.sh     # stripped kernel smoke (m80-ci9i.6)
#   M80_SMOKE_MODE=snapshot ./scripts/smoke.sh      # snapshot capture+restore round-trip (m80-rrp.3.14)
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
#   M80_SMOKE_MODE            (default default; or "snapshot")

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

SMOKE_MODE="${M80_SMOKE_MODE:-default}"
case "$SMOKE_MODE" in
    default|snapshot) ;;
    *) echo "M80_SMOKE_MODE must be default|snapshot, got: $SMOKE_MODE" >&2; exit 1 ;;
esac

RUN_ROOT="${M80_RUN_ROOT:-/var/lib/m80-run}"
FIRECRACKER_BIN="${FIRECRACKER_BIN:-/opt/firecracker/bin/firecracker}"
JAILER_BIN="${JAILER_BIN:-/opt/firecracker/bin/jailer}"
IMAGE_BUILD_DIR="${IMAGE_BUILD_DIR:-/tmp/m80-build/$IMAGE_KIND}"
JAIL_UID="${M80_JAIL_UID:-$(id -u)}"
JAIL_GID="${M80_JAIL_GID:-$(getent group kvm | cut -d: -f3 || id -g)}"

FIRECRACKER_VERSION="${M80_FIRECRACKER_VERSION:-v1.15.1}"

mode="${1:-full}"

# --- resolve kernel image ---
# For stripped kernel kind, find the built artifact (requires prior
# Docker build of the m80-image-build kernel pipeline; see
# crates/m80-image-build/kernel-builder/ and m80-ci9i.2).
if [[ "$KERNEL_KIND" == "stripped" ]]; then
    if [[ -n "${M80_STRIPPED_KERNEL_PATH:-}" ]]; then
        KERNEL_IMAGE="$M80_STRIPPED_KERNEL_PATH"
    else
        # Use the newest built stripped kernel artifact.
        STRIPPED_GLOB="crates/m80-image-build/kernels/vmlinux-m80-*.bin"
        set +f
        # shellcheck disable=SC2086
        stripped_hits=( $STRIPPED_GLOB )
        set -f
        if [[ "${#stripped_hits[@]}" -eq 0 || ! -f "${stripped_hits[0]}" ]]; then
            echo "# stripped kernel not yet built; skipping (run m80-image-build kernel build first)"
            echo "# to build: see crates/m80-image-build/kernel-builder/Dockerfile (m80-ci9i.2)"
            exit 0
        fi
        KERNEL_IMAGE="$(ls -t "${stripped_hits[@]}" | head -n 1)"
    fi
    echo "# using stripped kernel: $KERNEL_IMAGE"
else
    KERNEL_IMAGE="${IMAGE_BUILD_DIR}/vmlinux"
fi

ROOTFS_IMAGE="${IMAGE_BUILD_DIR}/output.ext4"

echo "=== smoke config ==="
echo "  image-kind:  $IMAGE_KIND"
echo "  kernel-kind: $KERNEL_KIND"
echo "  smoke-mode:  $SMOKE_MODE"
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
    if [[ ! -f "$ROOTFS_IMAGE" || ! -f "${ROOTFS_IMAGE}.manifest.json" ]]; then
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
# Smoke mode: snapshot capture + restore round-trip (m80-rrp.3.14)
# =========================================================================
if [[ "$SMOKE_MODE" == "snapshot" ]]; then
    echo "=== snapshot smoke: cold launch (step 1/4) ==="
    # Step 1: cold launch, exec, record vm_id.
    vm_id_file="$(mktemp)"
    trap 'rm -f "$out_file" "$vm_id_file"' EXIT
    if ! timeout 90 sudo env "${M80_ENV[@]}" ./target/release/m80 launch \
            --network noegress -- /bin/echo snap-cold-ok \
            > "$out_file" 2>&1; then
        echo "=== SMOKE FAILED (cold launch) ==="
        tail -40 "$out_file"
        exit 1
    fi
    if ! grep -q "^snap-cold-ok$" "$out_file"; then
        echo "=== SMOKE FAILED (cold launch: expected 'snap-cold-ok' in stdout) ==="
        tail -40 "$out_file"
        exit 1
    fi
    echo "  cold launch OK"

    # Step 2: snapshot capture.
    # NOTE (v0.1): `m80 snapshot capture` requires in-process IPC and is a
    # v0.1 stub (same gap as `m80 exec`). The smoke wires the CLI invocation
    # so the design is exercised at the parsing layer; the full end-to-end
    # capture path awaits v0.2 out-of-process IPC or an in-process integration
    # test in crates/m80-firecracker/tests/snapshot_integration.rs.
    # See docs/perf/cold-launch.md "Snapshot smoke checkpoint" for details.
    SNAP_DIR="/tmp/m80-smoke-snap"
    sudo rm -rf "$SNAP_DIR"
    sudo mkdir -p "$SNAP_DIR"
    echo "=== snapshot smoke: capture step (v0.1 stub note — see cold-launch.md) ==="
    echo "# TODO (m80-rrp.3.14): m80 snapshot capture requires out-of-process IPC (v0.2)."
    echo "# The capture + restore end-to-end is exercised by the in-process integration"
    echo "# test at crates/m80-firecracker/tests/snapshot_integration.rs."
    echo "# Skipping live capture; proceeding to parse-layer CLI smoke only."

    # Exercise the CLI parse layer (stub exits with EXIT_NOT_IMPLEMENTED=7).
    # We consider exit 7 a pass here — it proves the flag parses correctly and
    # the error path is clean (not a panic or unknown-subcommand failure).
    capture_exit=0
    sudo env "${M80_ENV[@]}" ./target/release/m80 snapshot capture smoke-vm \
        --store-root "$SNAP_DIR" > "$out_file" 2>&1 || capture_exit=$?
    if [[ "$capture_exit" -ne 7 ]]; then
        echo "=== SMOKE FAILED (snapshot capture stub: expected exit 7, got $capture_exit) ==="
        tail -20 "$out_file"
        exit 1
    fi
    echo "  snapshot capture stub exit=7 OK (v0.1 IPC gap confirmed)"

    # Step 3: restore launch.
    # Without a real snapshot on disk, we exercise --from-snapshot path by
    # expecting it to fail with a clear "snapshot file not found" error
    # (exit 6 = Config error) rather than a panic or generic failure.
    echo "=== snapshot smoke: --from-snapshot parse + missing-file error (step 3/4) ==="
    restore_exit=0
    sudo env "${M80_ENV[@]}" ./target/release/m80 launch \
        --from-snapshot "$SNAP_DIR" -- /bin/echo restored \
        > "$out_file" 2>&1 || restore_exit=$?
    # Config error is exit 6 in m80-cli/src/errors.rs.
    if [[ "$restore_exit" -ne 6 ]]; then
        echo "=== SMOKE FAILED (--from-snapshot missing files: expected exit 6, got $restore_exit) ==="
        tail -20 "$out_file"
        exit 1
    fi
    echo "  --from-snapshot missing-file error exit=6 OK"

    echo "=== SNAPSHOT SMOKE PASSED (parse + stub layer) ==="
    echo "# Full capture+restore end-to-end: see crates/m80-firecracker/tests/snapshot_integration.rs"
    echo "# Bench numbers: TBD — pending KVM-exercised snapshot round-trip (m80-rrp.3.6)"
    exit 0
fi

# =========================================================================
# Default smoke: cold launch + exec + stop
# =========================================================================
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
