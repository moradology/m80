#!/usr/bin/env bash
# Prepare this L0 host to spawn ephemeral nested-KVM L1 runners.

set -Eeuo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DRY_RUN=0
JSON=0
INSTALL_PACKAGES=1
TARGET_USER="${SUDO_USER:-${USER:-}}"

usage() {
    cat <<'EOF'
Usage: scripts/setup-privileged-runner.sh [--dry-run] [--json] [--no-apt] [--user NAME]

Idempotently prepares the bare-metal host for m80 nested-virt release runners:
installs qemu/libvirt/cloud-init tooling, loads required kernel modules, starts
libvirt, ensures the default NAT network is active, and checks /dev/kvm plus
/dev/net/tun access.

Options:
  --dry-run    Print actions without changing the host.
  --json       Emit a machine-readable summary.
  --no-apt     Do not install missing OS packages.
  --user NAME  User to add to libvirt/kvm groups, default SUDO_USER or USER.
EOF
}

log() {
    if [[ "$JSON" -ne 1 ]]; then
        printf '%s\n' "$*"
    fi
}

run() {
    if [[ "$DRY_RUN" -eq 1 ]]; then
        log "dry-run: $*"
        return 0
    fi
    if [[ "$JSON" -eq 1 ]]; then
        "$@" >&2
    else
        "$@"
    fi
}

need_root_for_mutation() {
    if [[ "$DRY_RUN" -eq 1 ]]; then
        return 0
    fi
    if [[ "$(id -u)" -eq 0 ]]; then
        return 0
    fi
    if sudo -n true >/dev/null 2>&1; then
        return 0
    fi
    echo "setup-privileged-runner: root or passwordless sudo is required" >&2
    exit 1
}

as_root() {
    if [[ "$(id -u)" -eq 0 ]]; then
        run "$@"
    else
        run sudo -n "$@"
    fi
}

virsh_system() {
    if virsh --connect qemu:///system list --all >/dev/null 2>&1; then
        virsh --connect qemu:///system "$@"
    else
        sudo -n virsh --connect qemu:///system "$@"
    fi
}

have_command() {
    command -v "$1" >/dev/null 2>&1
}

install_packages() {
    if [[ "$INSTALL_PACKAGES" -ne 1 ]]; then
        return 0
    fi
    local missing=()
    local package
    for package in qemu-system-x86 qemu-utils libvirt-daemon-system libvirt-clients virtinst cloud-image-utils dnsmasq-base bridge-utils iproute2 iptables ipset linux-headers-generic build-essential; do
        if ! dpkg-query -W -f='${Status}' "$package" 2>/dev/null | grep -q 'install ok installed'; then
            missing+=("$package")
        fi
    done
    if [[ "${#missing[@]}" -eq 0 ]]; then
        log "packages already installed"
        return 0
    fi
    log "installing packages: ${missing[*]}"
    as_root apt-get update
    as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y "${missing[@]}"
}

load_modules() {
    local cpu_vendor_module=
    if grep -qw svm /proc/cpuinfo; then
        cpu_vendor_module=kvm_amd
    elif grep -qw vmx /proc/cpuinfo; then
        cpu_vendor_module=kvm_intel
    fi

    local module
    for module in kvm "$cpu_vendor_module" tun bridge vhost_vsock; do
        [[ -n "$module" ]] || continue
        if lsmod | awk '{print $1}' | grep -qx "$module"; then
            log "module already loaded: $module"
        else
            log "loading module: $module"
            as_root modprobe "$module"
        fi
    done
}

start_libvirt() {
    local unit
    for unit in virtqemud libvirtd; do
        if systemctl list-unit-files "$unit.service" >/dev/null 2>&1; then
            log "starting $unit"
            as_root systemctl enable --now "$unit.service"
        fi
    done
}

activate_default_network() {
    if ! virsh_system net-info default >/dev/null 2>&1; then
        log "libvirt default network is not defined; libvirt package should define it"
        return 0
    fi
    if ! virsh_system net-info default | grep -q '^Active:.*yes'; then
        log "starting libvirt default network"
        virsh_system net-start default
    fi
    virsh_system net-autostart default >/dev/null
}

ensure_groups() {
    [[ -n "$TARGET_USER" ]] || return 0
    local group
    for group in kvm libvirt; do
        if getent group "$group" >/dev/null 2>&1; then
            if id -nG "$TARGET_USER" | tr ' ' '\n' | grep -qx "$group"; then
                log "user $TARGET_USER already in group $group"
            else
                log "adding $TARGET_USER to group $group"
                as_root usermod -aG "$group" "$TARGET_USER"
            fi
        fi
    done
}

fix_device_modes() {
    if [[ -e /dev/kvm ]]; then
        as_root chgrp kvm /dev/kvm || true
        as_root chmod 0660 /dev/kvm || true
    fi
    if [[ -e /dev/net/tun ]]; then
        as_root chmod 0666 /dev/net/tun || true
    fi
}

verify() {
    local failures=()
    for cmd in virsh virt-install qemu-system-x86_64 qemu-img cloud-localds ssh ssh-keygen ip iptables ipset make cc; do
        if ! have_command "$cmd"; then
            failures+=("missing command: $cmd")
        fi
    done
    if [[ ! -e /dev/kvm ]]; then
        failures+=("missing /dev/kvm")
    elif [[ ! -r /dev/kvm || ! -w /dev/kvm ]]; then
        failures+=("/dev/kvm is not rw for current user")
    fi
    if [[ ! -e /dev/net/tun ]]; then
        failures+=("missing /dev/net/tun")
    fi
    if [[ -r /sys/module/kvm_amd/parameters/nested ]]; then
        if ! grep -Eq '^(1|Y|y)$' /sys/module/kvm_amd/parameters/nested; then
            failures+=("kvm_amd nested mode is not enabled")
        fi
    elif [[ -r /sys/module/kvm_intel/parameters/nested ]]; then
        if ! grep -Eq '^(1|Y|y)$' /sys/module/kvm_intel/parameters/nested; then
            failures+=("kvm_intel nested mode is not enabled")
        fi
    else
        failures+=("no KVM nested parameter found")
    fi
    if ! virsh_system list --all >/dev/null 2>&1; then
        failures+=("cannot connect to qemu:///system")
    fi
    if [[ "${#failures[@]}" -gt 0 ]]; then
        printf 'setup-privileged-runner failed:\n' >&2
        printf '  - %s\n' "${failures[@]}" >&2
        return 1
    fi
}

emit_json() {
    python3 - "$ROOT_DIR" "$TARGET_USER" <<'PY'
import json
import os
import pathlib
import shutil
import subprocess
import sys

def read(path):
    p = pathlib.Path(path)
    return p.read_text().strip() if p.exists() else None

payload = {
    "schema_version": 1,
    "workspace": sys.argv[1],
    "target_user": sys.argv[2] or None,
    "commands": {cmd: shutil.which(cmd) for cmd in [
        "virsh", "virt-install", "qemu-system-x86_64", "qemu-img", "cloud-localds"
    ]},
    "devices": {
        "kvm": pathlib.Path("/dev/kvm").exists(),
        "tun": pathlib.Path("/dev/net/tun").exists(),
    },
    "nested": {
        "amd": read("/sys/module/kvm_amd/parameters/nested"),
        "intel": read("/sys/module/kvm_intel/parameters/nested"),
    },
}
try:
    try:
        subprocess.run(["virsh", "--connect", "qemu:///system", "list", "--all"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        payload["libvirt_system"] = "connected"
    except Exception:
        subprocess.run(["sudo", "-n", "virsh", "--connect", "qemu:///system", "list", "--all"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        payload["libvirt_system"] = "connected-via-sudo"
except Exception as exc:
    payload["libvirt_system"] = f"unavailable: {exc}"
print(json.dumps(payload, indent=2, sort_keys=True))
PY
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --json)
            JSON=1
            shift
            ;;
        --no-apt)
            INSTALL_PACKAGES=0
            shift
            ;;
        --user)
            TARGET_USER="${2:?--user requires a value}"
            shift 2
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

need_root_for_mutation
install_packages
load_modules
start_libvirt
activate_default_network
ensure_groups
fix_device_modes
verify

if [[ "$JSON" -eq 1 ]]; then
    emit_json
else
    log "privileged runner host is ready"
fi
