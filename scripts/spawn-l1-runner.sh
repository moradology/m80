#!/usr/bin/env bash
# Spawn and manage an ephemeral libvirt L1 VM with nested KVM exposed to m80.

set -Eeuo pipefail

LIBVIRT_URI="${M80_L1_LIBVIRT_URI:-qemu:///system}"
WORK_ROOT="${M80_L1_WORK_ROOT:-/tank/tmp/m80-l1-runner}"
NAME="${M80_L1_NAME:-m80-l1-runner}"
MEMORY_MIB="${M80_L1_MEMORY_MIB:-8192}"
VCPUS="${M80_L1_VCPUS:-4}"
DISK_SIZE="${M80_L1_DISK_SIZE:-40G}"
IMAGE_URL="${M80_L1_IMAGE_URL:-https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img}"
IMAGE_SHA256="${M80_L1_IMAGE_SHA256:-}"
SSH_USER="${M80_L1_USER:-m80}"
SSH_KEY="${M80_L1_SSH_KEY:-}"
FIRECRACKER_VERSION="${M80_FIRECRACKER_VERSION:-v1.15.1}"
WAIT_SECONDS="${M80_L1_WAIT_SECONDS:-420}"
LIBVIRT_PREFIX=()

usage() {
    cat <<'EOF'
Usage:
  scripts/spawn-l1-runner.sh create [options]
  scripts/spawn-l1-runner.sh wait [options]
  scripts/spawn-l1-runner.sh ssh [options] -- COMMAND...
  scripts/spawn-l1-runner.sh destroy [options]
  scripts/spawn-l1-runner.sh info [options]

Options:
  --name NAME          libvirt domain name, default m80-l1-runner
  --work-root PATH     state/cache root, default /tank/tmp/m80-l1-runner
  --memory MiB         L1 memory, default 8192
  --vcpus N            L1 vCPU count, default 4
  --disk-size SIZE     qcow2 overlay size, default 40G
  --image-url URL      Ubuntu cloud image URL
  --ssh-key PATH       private key for the L1 user
  --wait-seconds N     create/wait timeout, default 420

create prints connection details after cloud-init, /dev/kvm, svm, and
Firecracker/jailer checks pass. destroy removes the domain and this runner's
state directory.
EOF
}

die() {
    echo "spawn-l1-runner: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "missing required command: $1; run scripts/setup-privileged-runner.sh first"
}

state_dir() {
    printf '%s/%s\n' "$WORK_ROOT" "$NAME"
}

base_image() {
    printf '%s/cache/%s\n' "$WORK_ROOT" "$(basename "$IMAGE_URL")"
}

domain_disk() {
    printf '%s/disk.qcow2\n' "$(state_dir)"
}

seed_iso() {
    printf '%s/seed.iso\n' "$(state_dir)"
}

ssh_pubkey() {
    printf '%s.pub\n' "$SSH_KEY"
}

ssh_base() {
    local ip="$1"
    shift || true
    ssh -i "$SSH_KEY" \
        -o StrictHostKeyChecking=no \
        -o UserKnownHostsFile="$(state_dir)/known_hosts" \
        -o ConnectTimeout=8 \
        "$SSH_USER@$ip" "$@"
}

init_libvirt_prefix() {
    if virsh --connect "$LIBVIRT_URI" list --all >/dev/null 2>&1; then
        LIBVIRT_PREFIX=()
        return 0
    fi
    if sudo -n virsh --connect "$LIBVIRT_URI" list --all >/dev/null 2>&1; then
        LIBVIRT_PREFIX=(sudo -n)
        return 0
    fi
    die "cannot connect to $LIBVIRT_URI; run scripts/setup-privileged-runner.sh first"
}

virsh_l() {
    "${LIBVIRT_PREFIX[@]}" virsh --connect "$LIBVIRT_URI" "$@"
}

virt_install_l() {
    "${LIBVIRT_PREFIX[@]}" virt-install --connect "$LIBVIRT_URI" "$@"
}

parse_common_options() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --name)
                NAME="${2:?--name requires a value}"
                shift 2
                ;;
            --work-root)
                WORK_ROOT="${2:?--work-root requires a value}"
                shift 2
                ;;
            --memory)
                MEMORY_MIB="${2:?--memory requires a value}"
                shift 2
                ;;
            --vcpus)
                VCPUS="${2:?--vcpus requires a value}"
                shift 2
                ;;
            --disk-size)
                DISK_SIZE="${2:?--disk-size requires a value}"
                shift 2
                ;;
            --image-url)
                IMAGE_URL="${2:?--image-url requires a value}"
                shift 2
                ;;
            --ssh-key)
                SSH_KEY="${2:?--ssh-key requires a value}"
                shift 2
                ;;
            --wait-seconds)
                WAIT_SECONDS="${2:?--wait-seconds requires a value}"
                shift 2
                ;;
            --)
                shift
                REMAINING_ARGS=("$@")
                return 0
                ;;
            *)
                REMAINING_ARGS=("$@")
                return 0
                ;;
        esac
    done
    REMAINING_ARGS=()
}

ensure_ssh_key() {
    if [[ -z "$SSH_KEY" ]]; then
        SSH_KEY="$WORK_ROOT/id_ed25519"
    fi
    mkdir -p "$(dirname "$SSH_KEY")"
    if [[ ! -f "$SSH_KEY" ]]; then
        ssh-keygen -q -t ed25519 -N '' -f "$SSH_KEY" -C "m80-l1-runner-$NAME"
    fi
}

download_base_image() {
    mkdir -p "$WORK_ROOT/cache"
    local image
    image="$(base_image)"
    if [[ ! -f "$image" ]]; then
        curl -fL --connect-timeout 10 --max-time 600 --retry 2 --retry-delay 1 -o "$image.tmp" "$IMAGE_URL"
        mv "$image.tmp" "$image"
    fi
    if [[ -n "$IMAGE_SHA256" ]]; then
        printf '%s  %s\n' "$IMAGE_SHA256" "$image" | sha256sum -c -
    fi
}

write_cloud_init() {
    local dir="$1"
    local pubkey
    pubkey="$(cat "$(ssh_pubkey)")"
    cat >"$dir/meta-data" <<EOF
instance-id: $NAME
local-hostname: $NAME
EOF
    cat >"$dir/user-data" <<EOF
#cloud-config
package_update: true
packages:
  - ca-certificates
  - curl
  - e2fsprogs
  - iproute2
  - jq
  - openssh-server
  - procps
  - python3
  - sudo
  - tar
  - util-linux
users:
  - name: $SSH_USER
    gecos: m80 L1 runner
    groups: [adm, sudo, kvm]
    shell: /bin/bash
    sudo: ALL=(ALL) NOPASSWD:ALL
    lock_passwd: true
    ssh_authorized_keys:
      - $pubkey
write_files:
  - path: /usr/local/sbin/install-firecracker-train.sh
    owner: root:root
    permissions: '0755'
    content: |
      #!/usr/bin/env bash
      set -Eeuo pipefail
      version="\${1:-$FIRECRACKER_VERSION}"
      arch="x86_64"
      work="\$(mktemp -d)"
      trap 'rm -rf "\$work"' EXIT
      url="https://github.com/firecracker-microvm/firecracker/releases/download/\${version}/firecracker-\${version}-\${arch}.tgz"
      curl -fL --connect-timeout 10 --max-time 300 --retry 2 --retry-delay 1 -o "\$work/firecracker.tgz" "\$url"
      tar -xzf "\$work/firecracker.tgz" -C "\$work"
      dir="\$work/release-\${version}-\${arch}"
      install -d -m 0755 /opt/firecracker/bin
      install -o root -g root -m 0755 "\$dir/firecracker-\${version}-\${arch}" /opt/firecracker/bin/firecracker
      install -o root -g root -m 0755 "\$dir/jailer-\${version}-\${arch}" /opt/firecracker/bin/jailer
      install -o root -g root -m 0644 "\$dir/seccomp-filter-\${version}-\${arch}.json" /opt/firecracker/bin/firecracker-seccomp-filter.json
      "\$dir/seccompiler-bin-\${version}-\${arch}" \\
        --target-arch x86_64 \\
        --input-file /opt/firecracker/bin/firecracker-seccomp-filter.json \\
        --output-file /opt/firecracker/bin/firecracker-seccomp-filter.bin
      chown root:root /opt/firecracker/bin/firecracker-seccomp-filter.bin
      chmod 0644 /opt/firecracker/bin/firecracker-seccomp-filter.bin
      /opt/firecracker/bin/firecracker --version
      /opt/firecracker/bin/jailer --version
runcmd:
  - [ bash, -lc, 'getent group 3000 >/dev/null || groupadd --system --gid 3000 m80jail' ]
  - [ bash, -lc, 'getent passwd 3000 >/dev/null || useradd --system --uid 3000 --gid 3000 --home-dir /nonexistent --shell /usr/sbin/nologin --no-create-home m80jail' ]
  - [ bash, -lc, 'modprobe kvm_amd || modprobe kvm_intel || true' ]
  - [ bash, -lc, 'modprobe nf_conntrack || true' ]
  - [ bash, -lc, 'modprobe br_netfilter || true' ]
  - [ bash, -lc, 'modprobe bridge || true' ]
  - [ bash, -lc, 'modprobe tap || true' ]
  - [ bash, -lc, 'modprobe vhost_vsock || true' ]
  - [ bash, -lc, 'modprobe tun || true' ]
  - [ bash, -lc, '/usr/local/sbin/install-firecracker-train.sh $FIRECRACKER_VERSION' ]
  - [ bash, -lc, 'test -e /dev/kvm && chmod 0660 /dev/kvm || true' ]
  - [ bash, -lc, 'test -e /dev/net/tun && chmod 0666 /dev/net/tun || true' ]
EOF
}

create_domain() {
    init_libvirt_prefix
    require_command virsh
    require_command virt-install
    require_command qemu-img
    require_command cloud-localds
    require_command curl
    require_command ssh-keygen

    if virsh_l dominfo "$NAME" >/dev/null 2>&1; then
        die "domain already exists: $NAME"
    fi
    ensure_ssh_key
    download_base_image

    local dir
    dir="$(state_dir)"
    mkdir -p "$dir"
    write_cloud_init "$dir"
    cloud-localds "$(seed_iso)" "$dir/user-data" "$dir/meta-data"
    qemu-img create -f qcow2 -F qcow2 -b "$(base_image)" "$(domain_disk)" "$DISK_SIZE" >/dev/null

    if virsh_l net-info default >/dev/null 2>&1; then
        if ! virsh_l net-info default | grep -q '^Active:.*yes'; then
            virsh_l net-start default >/dev/null
        fi
    fi

    virt_install_l \
        --name "$NAME" \
        --memory "$MEMORY_MIB" \
        --vcpus "$VCPUS" \
        --cpu host-passthrough \
        --virt-type kvm \
        --import \
        --os-variant ubuntu24.04 \
        --disk "path=$(domain_disk),format=qcow2,bus=virtio" \
        --disk "path=$(seed_iso),device=cdrom" \
        --network network=default,model=virtio \
        --graphics none \
        --console pty,target_type=serial \
        --noautoconsole \
        --boot hd \
        >&2

    wait_domain
}

domain_mac() {
    virsh_l domiflist "$NAME" | awk '$3 == "default" {print $5; exit}'
}

domain_ip() {
    local mac="$1"
    virsh_l net-dhcp-leases default --mac "$mac" 2>/dev/null \
        | awk '/ipv4/ {split($5, a, "/"); print a[1]; exit}'
}

wait_for_ip() {
    init_libvirt_prefix
    local deadline=$((SECONDS + WAIT_SECONDS))
    local mac ip
    mac="$(domain_mac)"
    [[ -n "$mac" ]] || die "could not determine domain MAC for $NAME"
    while (( SECONDS < deadline )); do
        ip="$(domain_ip "$mac")"
        if [[ -n "$ip" ]]; then
            printf '%s\n' "$ip"
            return 0
        fi
        sleep 2
    done
    die "timed out waiting for DHCP lease for $NAME"
}

wait_domain() {
    init_libvirt_prefix
    require_command ssh
    local ip deadline
    ip="$(wait_for_ip)"
    deadline=$((SECONDS + WAIT_SECONDS))
    while (( SECONDS < deadline )); do
        if ssh_base "$ip" true >/dev/null 2>&1; then
            break
        fi
        sleep 3
    done
    if ! ssh_base "$ip" true >/dev/null 2>&1; then
        die "timed out waiting for SSH on $SSH_USER@$ip"
    fi
    ssh_base "$ip" 'sudo cloud-init status --wait >/dev/null'
    ssh_base "$ip" 'test -r /dev/kvm && test -w /dev/kvm'
    ssh_base "$ip" "grep -qw svm /proc/cpuinfo || grep -qw vmx /proc/cpuinfo"
    ssh_base "$ip" 'getent passwd 3000 >/dev/null && getent group 3000 >/dev/null'
    ssh_base "$ip" 'test -d /sys/module/nf_conntrack || grep -qw nf_conntrack /proc/modules'
    ssh_base "$ip" 'test -d /sys/module/br_netfilter || grep -qw br_netfilter /proc/modules'
    ssh_base "$ip" 'test -d /sys/module/tap || grep -qw tap /proc/modules'
    ssh_base "$ip" '/opt/firecracker/bin/firecracker --version | grep -q "Firecracker v"'
    ssh_base "$ip" '/opt/firecracker/bin/jailer --version | grep -q "Jailer v"'
    ssh_base "$ip" 'test -s /opt/firecracker/bin/firecracker-seccomp-filter.bin'
    write_info "$ip"
    cat "$(state_dir)/runner.env"
}

write_info() {
    local ip="$1"
    local ssh_opts
    ssh_opts="-i $SSH_KEY -o StrictHostKeyChecking=no -o UserKnownHostsFile=$(state_dir)/known_hosts"
    {
        printf 'M80_L1_NAME=%q\n' "$NAME"
        printf 'M80_L1_IP=%q\n' "$ip"
        printf 'M80_L1_USER=%q\n' "$SSH_USER"
        printf 'M80_L1_SSH_KEY=%q\n' "$SSH_KEY"
        printf 'M80_L1_SSH_TARGET=%q\n' "$SSH_USER@$ip"
        printf 'M80_L1_SSH_OPTS=%q\n' "$ssh_opts"
    } >"$(state_dir)/runner.env"
}

current_ip() {
    if [[ -f "$(state_dir)/runner.env" ]]; then
        # shellcheck disable=SC1090,SC1091
        source "$(state_dir)/runner.env"
        if [[ -n "${M80_L1_IP:-}" ]]; then
            printf '%s\n' "$M80_L1_IP"
            return 0
        fi
    fi
    wait_for_ip
}

ssh_command() {
    local ip
    ip="$(current_ip)"
    if [[ "${#REMAINING_ARGS[@]}" -eq 0 ]]; then
        ssh_base "$ip"
    else
        ssh_base "$ip" "${REMAINING_ARGS[@]}"
    fi
}

destroy_domain() {
    init_libvirt_prefix
    if virsh_l dominfo "$NAME" >/dev/null 2>&1; then
        if virsh_l domstate "$NAME" | grep -q running; then
            virsh_l destroy "$NAME" >/dev/null || true
        fi
        virsh_l undefine "$NAME" --nvram --remove-all-storage >/dev/null 2>&1 || \
            virsh_l undefine "$NAME" --nvram >/dev/null 2>&1 || true
    fi
    rm -rf "$(state_dir)"
}

info_domain() {
    local ip
    ip="$(current_ip)"
    python3 - "$NAME" "$ip" "$SSH_USER" "$SSH_KEY" "$WORK_ROOT" <<'PY'
import json
import sys
print(json.dumps({
    "schema_version": 1,
    "name": sys.argv[1],
    "ip": sys.argv[2],
    "ssh_user": sys.argv[3],
    "ssh_key": sys.argv[4],
    "work_root": sys.argv[5],
    "ssh_target": f"{sys.argv[3]}@{sys.argv[2]}",
}, indent=2, sort_keys=True))
PY
}

[[ $# -gt 0 ]] || { usage >&2; exit 2; }
COMMAND="$1"
shift
REMAINING_ARGS=()
parse_common_options "$@"
if [[ -z "$SSH_KEY" ]]; then
    SSH_KEY="$WORK_ROOT/id_ed25519"
fi

case "$COMMAND" in
    create)
        create_domain
        ;;
    wait)
        wait_domain
        ;;
    ssh|exec)
        ssh_command
        ;;
    destroy)
        destroy_domain
        ;;
    info)
        info_domain
        ;;
    -h|--help|help)
        usage
        ;;
    *)
        die "unknown command: $COMMAND"
        ;;
esac
