#!/usr/bin/env bash
# Run scripts/smoke.sh with /usr/bin/systemd-run masked so preflight selects
# the wrapper launch path on a systemd host.
set -euo pipefail

mode="${1:-launch-only}"
case "$mode" in
    full|launch-only) ;;
    *) echo "mode must be full or launch-only, got: $mode" >&2; exit 2 ;;
esac

repo_root="$(git rev-parse --show-toplevel)"
runner_user="$(id -un)"
runner_path="$PATH"
tmp_root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
tmp_dir="$(mktemp -d "$tmp_root/m80-wrapper-smoke.XXXXXX")"
trap 'rm -rf "$tmp_dir"' EXIT

mask="$tmp_dir/systemd-run"
cat >"$mask" <<'SH'
#!/bin/sh
echo systemd-run masked for wrapper smoke >&2
exit 127
SH
chmod 0755 "$mask"

env_args=()
for name in \
    M80_RUN_ROOT \
    M80_ARTIFACT_DIR \
    M80_FIRECRACKER_BIN \
    M80_FIRECRACKER_SECCOMP_FILTER \
    M80_JAILER_BIN \
    M80_JAILER_HARDEN_BIN \
    M80_NET_HELPER_BIN \
    M80_KERNEL_IMAGE \
    M80_ROOTFS_IMAGE \
    M80_KERNEL_KIND \
    M80_IMAGE_KIND \
    M80_STRIPPED_KERNEL_PATH \
    M80_GUESTD_ARTIFACT \
    M80_JAIL_UID \
    M80_JAIL_GID \
    M80_FIRECRACKER_VERSION \
    M80_SKIP_CHECK_KSM \
    M80_SKIP_CHECK_SMT \
    M80_SMT_CHECK \
    M80_SKIP_CHECK_SWAP \
    M80_SKIP_CHECK_NESTED_VIRT \
    M80_SKIP_CHECK_KVM_TIMER \
    M80_SKIP_CHECK_CGROUP_FAVORDYNMODS
do
    if [[ -v "$name" ]]; then
        env_args+=("$name=${!name}")
    fi
done

if [[ ! -e /usr/bin/systemd-run ]]; then
    exec env PATH="$runner_path" "${env_args[@]}" "$repo_root/scripts/smoke.sh" "$mode"
fi

sudo -n unshare --mount --propagation private bash -c '
set -euo pipefail
mask="$1"
runner_user="$2"
runner_path="$3"
repo_root="$4"
mode="$5"
shift 5
mount --bind "$mask" /usr/bin/systemd-run
cd "$repo_root"
exec runuser -u "$runner_user" -- env PATH="$runner_path" "$@" "$repo_root/scripts/smoke.sh" "$mode"
' bash "$mask" "$runner_user" "$runner_path" "$repo_root" "$mode" "${env_args[@]}"
