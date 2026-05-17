#!/usr/bin/env bash
# Real-KVM density smoke for Phase C Shared pmem.

set -euo pipefail

cd "$(dirname "$0")/.."

VM_COUNT="${M80_PMEM_SHARED_VM_COUNT:-4}"
CYCLES="${M80_PMEM_SHARED_CYCLES:-10}"
PAYLOAD_MIB="${M80_PMEM_SHARED_PAYLOAD_MIB:-128}"
PER_VM_OVERHEAD_KIB="${M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB:-131072}"
RUN_ROOT="${M80_RUN_ROOT:-/var/lib/m80-psd}"
ARTIFACT="${M80_PMEM_SHARED_DENSITY_ARTIFACT:-docs/perf/pmem-shared-density.md}"
if [[ "$ARTIFACT" = /* ]]; then
    ARTIFACT_PATH="$ARTIFACT"
else
    ARTIFACT_PATH="$PWD/$ARTIFACT"
fi
ALLOW_OTHER_VMS="${M80_PMEM_SHARED_ALLOW_OTHER_VMS:-0}"
FIRECRACKER_BIN="${M80_FIRECRACKER_BIN:-/opt/firecracker/bin/firecracker}"
JAILER_BIN="${M80_JAILER_BIN:-/opt/firecracker/bin/jailer}"
JAIL_UID="${M80_JAIL_UID:-$(id -u)}"
JAIL_GID="${M80_JAIL_GID:-$(id -g)}"
FIRECRACKER_VERSION="${M80_FIRECRACKER_VERSION:-v1.15.1}"
KERNEL_KIND="${M80_KERNEL_KIND:-stripped}"

if [[ "$KERNEL_KIND" != "stripped" ]]; then
    echo "M80_KERNEL_KIND must be stripped for pmem erofs+DAX density, got: $KERNEL_KIND" >&2
    exit 2
fi

if [[ -n "${M80_KERNEL_IMAGE:-}" ]]; then
    KERNEL_IMAGE="$M80_KERNEL_IMAGE"
elif [[ -n "${M80_STRIPPED_KERNEL_PATH:-}" ]]; then
    KERNEL_IMAGE="$M80_STRIPPED_KERNEL_PATH"
else
    shopt -s nullglob
    kernels=(crates/m80-image-build/kernels/vmlinux-m80-*.bin)
    shopt -u nullglob
    if [[ "${#kernels[@]}" -eq 0 ]]; then
        echo "no stripped kernel found; set M80_KERNEL_IMAGE" >&2
        exit 2
    fi
    KERNEL_IMAGE="${kernels[0]}"
    for candidate in "${kernels[@]}"; do
        if [[ "$candidate" -nt "$KERNEL_IMAGE" ]]; then
            KERNEL_IMAGE="$candidate"
        fi
    done
fi

ROOTFS_IMAGE="${M80_ROOTFS_IMAGE:-/tank/tmp/m80-build/shared-pmem-ubuntu2/output.ext4}"

for path in "$FIRECRACKER_BIN" "$JAILER_BIN" "$KERNEL_IMAGE" "$ROOTFS_IMAGE"; do
    if [[ ! -e "$path" ]]; then
        echo "missing required path: $path" >&2
        exit 2
    fi
done

FIRECRACKER_BIN="$(realpath "$FIRECRACKER_BIN")"
JAILER_BIN="$(realpath "$JAILER_BIN")"
KERNEL_IMAGE="$(realpath "$KERNEL_IMAGE")"
ROOTFS_IMAGE="$(realpath "$ROOTFS_IMAGE")"

if [[ "$ALLOW_OTHER_VMS" != "1" ]]; then
    existing_firecrackers="$(pgrep -af '(^|/)firecracker( |$)' || true)"
    if [[ -n "$existing_firecrackers" ]]; then
        echo "refusing density measurement while other Firecracker VMs are present" >&2
        echo "$existing_firecrackers" >&2
        echo "stop them, or set M80_PMEM_SHARED_ALLOW_OTHER_VMS=1 for a non-closeable diagnostic run" >&2
        exit 3
    fi
fi

sudo -n true >/dev/null
sudo -n mkdir -p "$RUN_ROOT"

cargo test -p m80-firecracker --test pmem_shared_host_page_sharing_real_kvm --no-run
test_bin="$(
    find target/debug/deps -maxdepth 1 -type f -executable \
        -name 'pmem_shared_host_page_sharing_real_kvm-*' | sort | tail -n 1
)"
if [[ -z "$test_bin" ]]; then
    echo "pmem Shared density test binary not found" >&2
    exit 1
fi

repro_command="M80_PMEM_SHARED_ALLOW_OTHER_VMS=$ALLOW_OTHER_VMS"
repro_command+=" M80_PMEM_SHARED_VM_COUNT=$VM_COUNT"
repro_command+=" M80_PMEM_SHARED_CYCLES=$CYCLES"
repro_command+=" M80_PMEM_SHARED_PAYLOAD_MIB=$PAYLOAD_MIB"
repro_command+=" M80_PMEM_SHARED_DENSITY_ARTIFACT=$ARTIFACT"
repro_command+=" M80_RUN_ROOT=$RUN_ROOT"
repro_command+=" M80_KERNEL_IMAGE=$KERNEL_IMAGE"
repro_command+=" M80_ROOTFS_IMAGE=$ROOTFS_IMAGE"
repro_command+=" M80_FIRECRACKER_BIN=$FIRECRACKER_BIN"
repro_command+=" M80_JAILER_BIN=$JAILER_BIN"
repro_command+=" $0"

timeout 1800 sudo -n env \
    M80_RUN_PMEM_SHARED_DENSITY=1 \
    M80_PMEM_SHARED_VM_COUNT="$VM_COUNT" \
    M80_PMEM_SHARED_CYCLES="$CYCLES" \
    M80_PMEM_SHARED_PAYLOAD_MIB="$PAYLOAD_MIB" \
    M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB="$PER_VM_OVERHEAD_KIB" \
    M80_PMEM_SHARED_ALLOW_OTHER_VMS="$ALLOW_OTHER_VMS" \
    M80_PMEM_SHARED_DENSITY_ARTIFACT="$ARTIFACT_PATH" \
    M80_PMEM_SHARED_REPRO_COMMAND="$repro_command" \
    M80_FIRECRACKER_BIN="$FIRECRACKER_BIN" \
    M80_JAILER_BIN="$JAILER_BIN" \
    M80_KERNEL_IMAGE="$KERNEL_IMAGE" \
    M80_KERNEL_KIND="$KERNEL_KIND" \
    M80_ROOTFS_IMAGE="$ROOTFS_IMAGE" \
    M80_RUN_ROOT="$RUN_ROOT" \
    M80_FIRECRACKER_VERSION="$FIRECRACKER_VERSION" \
    M80_JAIL_UID="$JAIL_UID" \
    M80_JAIL_GID="$JAIL_GID" \
    "$test_bin" shared_pmem_host_page_sharing_measurement_lives_in_density_gate \
    --ignored --exact --nocapture

echo "pmem Shared density artifact: $ARTIFACT"
