#!/usr/bin/env bash
# Validate a real published m80 release at increasing isolation levels.

set -Eeuo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPO="${M80_PUBLIC_RELEASE_REPO_FULL:-moradology/m80}"
TAG="${M80_RELEASE_TAG:-latest}"
LEVEL="a-c"
WORK_ROOT="${M80_VALIDATE_RELEASE_ROOT:-/tank/tmp/m80-release-validate.$(date -u +%Y%m%dT%H%M%SZ).$$}"
DIST_DIR=""
SKIP_DOWNLOAD=0
PROOF_OUT=""
L1_SSH_TARGET="${M80_L1_SSH_TARGET:-}"
L1_SSH_KEY="${M80_L1_SSH_KEY:-}"
L1_SSH_OPTS="${M80_L1_SSH_OPTS:-}"
L1_REMOTE_BASE="${M80_VALIDATE_L1_REMOTE_BASE:-/tmp}"
SPAWN_L1=0
SPAWNED_L1_WORK_ROOT=""
CLEANUP=1
CURL_BIN="${M80_VALIDATE_CURL:-curl}"

usage() {
    cat <<'EOF'
Usage: scripts/validate-release.sh --level a|b|c|d|e|a-c|all [options]

Levels:
  a    Download a published tag and verify release integrity, bundle, handoff.
  b    Resolve real public latest and run the bounded freshness verifier.
  c    Run the real install.sh into a temporary install root; tries no-sudo
       first, then uses sudo only for the published installer's live preflight
       gate when required.
  d    Run install.sh in a clean Ubuntu container dry-run lane.
  e    Run install + `m80 run -- echo hello` on a nested-KVM L1 over SSH.
  a-c  Run A, B, and C. This is the default local isolation gate.
  all  Run A, B, C, D, and E.

Options:
  --tag TAG             Release tag, or "latest" to resolve public latest.
  --repo OWNER/REPO     Public GitHub repo, default moradology/m80.
  --work-root PATH      Work directory, default /tank/tmp/m80-release-validate.*
  --dist-dir PATH       Use an existing downloaded dist directory.
  --skip-download       With --dist-dir, do not call gh release download.
  --proof-out PATH      Write level-E proof JSON to this path.
  --l1-ssh-target HOST  SSH target for level E, e.g. m80@192.0.2.10.
  --l1-ssh-key PATH     SSH private key for level E.
  --l1-ssh-opts TEXT    Extra SSH options for level E.
  --l1-remote-base PATH Remote scratch root for level E, default /tmp.
  --spawn-l1            Create/destroy an L1 with scripts/spawn-l1-runner.sh.
  --keep-work           Preserve local and remote work directories.
EOF
}

die() {
    echo "validate-release: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

json_quote() {
    python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"
}

resolve_tag() {
    if [[ "$TAG" != "latest" ]]; then
        printf '%s\n' "$TAG"
        return 0
    fi
    python3 scripts/stable_latest_bootstrap.py \
        --latest-url "https://api.github.com/repos/$REPO/releases/latest" \
        --json \
        | jq -r '.resolved_tag'
}

asset_url() {
    local tag="$1"
    local asset="$2"
    printf 'https://github.com/%s/releases/download/%s/%s\n' "$REPO" "$tag" "$asset"
}

required_assets() {
    cat <<'EOF'
install.sh
install.sh.sha256
SHA256SUMS
m80-linux-x86_64.tar.gz
m80-linux-x86_64.tar.gz.sha256
m80-linux-x86_64.bundle.json
m80-linux-x86_64.bundle.json.sha256
m80-release-assets.json
m80-release-assets.json.sha256
m80-bootstrap-selector.tsv
m80-bootstrap-selector.tsv.sha256
m80-release-build.json
m80-release-build.json.sha256
m80-release-integrity.json
m80-release-integrity.attestation.jsonl
m80-release-attestation.json
EOF
}

download_release_assets() {
    local tag="$1"
    local dist="$2"
    mkdir -p "$dist"
    if [[ "$SKIP_DOWNLOAD" -eq 1 ]]; then
        return 0
    fi
    if command -v gh >/dev/null 2>&1; then
        gh release download "$tag" --repo "$REPO" --dir "$dist" --clobber >/dev/null
        return 0
    fi
    require_command "$CURL_BIN"
    local asset
    while IFS= read -r asset; do
        [[ -n "$asset" ]] || continue
        "$CURL_BIN" -fL --connect-timeout 10 --max-time 300 --retry 2 --retry-delay 1 \
            -o "$dist/$asset" "$(asset_url "$tag" "$asset")"
    done < <(required_assets)
}

precheck_asset_set() {
    local dist="$1"
    local missing=()
    local asset
    while IFS= read -r asset; do
        [[ -n "$asset" ]] || continue
        if [[ ! -f "$dist/$asset" ]]; then
            missing+=("$asset")
        fi
    done < <(required_assets)
    if [[ "${#missing[@]}" -gt 0 ]]; then
        die "missing public release asset(s) in $dist: ${missing[*]}"
    fi

    local sidecar target expected actual
    for sidecar in "$dist"/*.sha256; do
        [[ -f "$sidecar" ]] || continue
        read -r expected target <"$sidecar"
        [[ -n "${expected:-}" && -n "${target:-}" ]] || die "malformed checksum sidecar: $sidecar"
        [[ "$target" != /* && "$target" != *..* ]] || die "unsafe checksum sidecar target in $sidecar: $target"
        [[ -f "$dist/$target" ]] || die "checksum sidecar $sidecar names missing target: $target"
        actual="$(sha256sum "$dist/$target" | awk '{print $1}')"
        if [[ "$actual" != "$expected" ]]; then
            die "sha256 mismatch for $target from $(basename "$sidecar"): expected $expected got $actual"
        fi
    done
}

level_a() {
    local tag="$1"
    local dist="$2"
    echo "level A: download and verify release assets for $tag"
    download_release_assets "$tag" "$dist"
    precheck_asset_set "$dist"
    local commit
    commit="$(python3 - "$dist/m80-release-integrity.json" <<'PY'
import json
import pathlib
import sys
print(json.loads(pathlib.Path(sys.argv[1]).read_text())["commit_sha"])
PY
)"
    python3 scripts/verify-release-integrity.py \
        "$dist/m80-release-integrity.json" \
        --dist-dir "$dist" \
        --release-tag "$tag" \
        --commit-sha "$commit" \
        --trust-policy docs/behaviors/release/m80-release-trust-policy.json \
        --attestation-bundle "$dist/m80-release-integrity.attestation.jsonl" \
        --attestation-metadata "$dist/m80-release-attestation.json" \
        --verification-time "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    python3 scripts/verify-release-bundle.py \
        "$dist/m80-linux-x86_64.tar.gz" \
        --release-tag "$tag" \
        --verify-sidecars \
        --downloaded-public-assets \
        --verify-integrity \
        --commit-sha "$commit" \
        --dist-dir "$dist" \
        --trust-policy docs/behaviors/release/m80-release-trust-policy.json
    python3 scripts/verify-install-handoff.py \
        "$dist" \
        --release-tag "$tag" \
        --trust-policy docs/behaviors/release/m80-release-trust-policy.json \
        --verification-time "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}

level_b() {
    echo "level B: public latest freshness"
    python3 scripts/release_freshness.py \
        --docs-root . \
        --json \
        --proof-out "$WORK_ROOT/freshness-proof.json" >/dev/null
    python3 scripts/release_freshness.py \
        --validate-proof "$WORK_ROOT/freshness-proof.json" >/dev/null
}

level_c() {
    local tag="$1"
    local dist="$2"
    echo "level C: install-root fixture for $tag"
    mkdir -p "$WORK_ROOT/install-fixture"
    set +e
    sh "$dist/install.sh" --install-root "$WORK_ROOT/install-fixture/install-root" \
        >"$WORK_ROOT/install-fixture/install-nosudo.stdout" \
        2>"$WORK_ROOT/install-fixture/install-nosudo.stderr"
    local nosudo_status=$?
    set -e
    local used_sudo=0
    if [[ "$nosudo_status" -ne 0 ]]; then
        if ! grep -q 'privilege_unavailable' "$WORK_ROOT/install-fixture/install-nosudo.stderr"; then
            cat "$WORK_ROOT/install-fixture/install-nosudo.stderr" >&2
            die "level C no-sudo install failed before the expected live preflight privilege gate"
        fi
        if ! sudo -n true >/dev/null 2>&1; then
            cat "$WORK_ROOT/install-fixture/install-nosudo.stderr" >&2
            die "level C needs sudo for this published installer's live preflight gate"
        fi
        used_sudo=1
        mkdir -p "$WORK_ROOT/install-fixture/tmp"
        set +e
        # Redirection intentionally stays in the caller shell; the fixture
        # output directory is owned by the invoking user.
        # shellcheck disable=SC2024
        sudo -n env \
            TMPDIR="$WORK_ROOT/install-fixture/tmp" \
            M80_JAIL_UID="$(id -u)" \
            M80_JAIL_GID="$(id -g)" \
            sh "$dist/install.sh" --install-root "$WORK_ROOT/install-fixture/install-root" \
            >"$WORK_ROOT/install-fixture/install.stdout" \
            2>"$WORK_ROOT/install-fixture/install.stderr"
        local sudo_status=$?
        set -e
        if [[ "$sudo_status" -ne 0 ]]; then
            cat "$WORK_ROOT/install-fixture/install.stderr" >&2
            die "level C sudo install retry failed"
        fi
    else
        cp "$WORK_ROOT/install-fixture/install-nosudo.stdout" "$WORK_ROOT/install-fixture/install.stdout"
        cp "$WORK_ROOT/install-fixture/install-nosudo.stderr" "$WORK_ROOT/install-fixture/install.stderr"
    fi
    "$WORK_ROOT/install-fixture/install-root/versions/$tag/bin/m80" \
        --json install-status \
        --install-root "$WORK_ROOT/install-fixture/install-root" \
        >"$WORK_ROOT/install-fixture/install-status.json"
    python3 - "$WORK_ROOT/install-fixture/level-c.json" "$nosudo_status" "$used_sudo" "$WORK_ROOT/install-fixture/install-root" <<'PY'
import json
import pathlib
import sys
path, nosudo_status, used_sudo, install_root = sys.argv[1:]
pathlib.Path(path).write_text(json.dumps({
    "schema_version": 1,
    "level": "c",
    "install_root": install_root,
    "unprivileged_attempt_exit_status": int(nosudo_status),
    "used_sudo_for_live_preflight": used_sudo == "1",
    "persistent_system_mutation": False,
}, indent=2, sort_keys=True) + "\n")
PY
}

level_d() {
    local dist="$1"
    echo "level D: container dry-run install"
    if ! command -v docker >/dev/null 2>&1; then
        die "docker is required for level D"
    fi
    docker run --rm \
        -v "$dist:/release:ro" \
        -v "$WORK_ROOT/container:/work" \
        ubuntu:24.04 \
        sh -ceu 'apt-get update >/dev/null && apt-get install -y ca-certificates curl python3 sudo tar coreutils >/dev/null && sh /release/install.sh --install-root /work/install-root --dry-run'
}

ssh_args() {
    local args=()
    if [[ -n "$L1_SSH_KEY" ]]; then
        args+=(-i "$L1_SSH_KEY")
    fi
    if [[ -n "$L1_SSH_OPTS" ]]; then
        # shellcheck disable=SC2206
        args+=($L1_SSH_OPTS)
    fi
    args+=(-o StrictHostKeyChecking=no)
    args+=(-o UserKnownHostsFile="$WORK_ROOT/l1-known-hosts")
    printf '%s\0' "${args[@]}"
}

l1_ssh() {
    local args=()
    while IFS= read -r -d '' item; do
        args+=("$item")
    done < <(ssh_args)
    # Remote commands are assembled by this script with quoted paths.
    # shellcheck disable=SC2029
    ssh "${args[@]}" "$L1_SSH_TARGET" "$@"
}

l1_scp_from() {
    local args=()
    while IFS= read -r -d '' item; do
        args+=("$item")
    done < <(ssh_args)
    scp "${args[@]}" "$L1_SSH_TARGET:$1" "$2"
}

level_e() {
    local tag="$1"
    [[ -n "$L1_SSH_TARGET" ]] || die "level E requires --l1-ssh-target or --spawn-l1"
    local remote_root
    remote_root="$L1_REMOTE_BASE/e$(date -u +%s)-$RANDOM"
    local local_proof="${PROOF_OUT:-docs/proofs/release/${tag}-nested-kvm-release-smoke.json}"
    mkdir -p "$(dirname "$local_proof")"
    echo "level E: nested-KVM public install smoke on $L1_SSH_TARGET for $tag"
    l1_ssh "mkdir -p '$remote_root/tmp' '$remote_root/home' '$remote_root/run'"
    l1_ssh "test -r /dev/kvm && test -w /dev/kvm && (grep -qw svm /proc/cpuinfo || grep -qw vmx /proc/cpuinfo)"
    l1_ssh "/opt/firecracker/bin/firecracker --version >/tmp/m80-fc-version.txt"
    l1_ssh "/opt/firecracker/bin/jailer --version >/tmp/m80-jailer-version.txt"
    l1_ssh "curl -fsSL '$(asset_url "$tag" install.sh)' -o '$remote_root/install.sh'"
    l1_ssh "sudo -n env TMPDIR='$remote_root/tmp' sh '$remote_root/install.sh' --install-root '$remote_root/install-root' >'$remote_root/install.stdout' 2>'$remote_root/install.stderr' || { cat '$remote_root/install.stderr' >&2; exit 1; }"
    l1_ssh "'$remote_root/install-root/versions/$tag/bin/m80' --json install-status --install-root '$remote_root/install-root' >'$remote_root/install-status.json'"
    local version_dir="$remote_root/install-root/versions/$tag"
    local artifact_dir="$version_dir/artifacts"
    set +e
    l1_ssh "sudo -n env HOME='$remote_root/home' TMPDIR='$remote_root/tmp' M80_DEFAULT_PROFILE=env M80_ARTIFACT_DIR='$artifact_dir' M80_KERNEL_IMAGE='$artifact_dir/vmlinux' M80_ROOTFS_IMAGE='$artifact_dir/output.ext4' M80_KERNEL_KIND=stock M80_RUN_ROOT='$remote_root/run' M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin M80_FIRECRACKER_VERSION=v1.15.1 M80_JAILER_BIN=/opt/firecracker/bin/jailer M80_JAILER_HARDEN_BIN='$version_dir/bin/m80-jailer-harden' M80_NET_HELPER_BIN='$version_dir/bin/m80-net-helper' M80_JAIL_UID=3000 M80_JAIL_GID=3000 timeout 180 '$version_dir/bin/m80' run -- echo hello >'$remote_root/run.stdout' 2>'$remote_root/run.stderr'"
    local run_status=$?
    set -e
    l1_ssh "pgrep -a firecracker >'$remote_root/firecracker-after.txt' 2>/dev/null || true"
    l1_ssh "sudo -n rm -rf '$remote_root/install-root'"
    l1_ssh "test ! -e '$remote_root/install-root' && echo removed >'$remote_root/install-root-removed.txt'"
    l1_scp_from "$remote_root/install.stdout" "$WORK_ROOT/install.stdout"
    l1_scp_from "$remote_root/install.stderr" "$WORK_ROOT/install.stderr"
    l1_scp_from "$remote_root/install-status.json" "$WORK_ROOT/install-status.json"
    l1_scp_from "$remote_root/run.stdout" "$WORK_ROOT/run.stdout"
    l1_scp_from "$remote_root/run.stderr" "$WORK_ROOT/run.stderr"
    l1_scp_from "$remote_root/firecracker-after.txt" "$WORK_ROOT/firecracker-after.txt"
    l1_scp_from "$remote_root/install-root-removed.txt" "$WORK_ROOT/install-root-removed.txt"
    l1_scp_from /tmp/m80-fc-version.txt "$WORK_ROOT/firecracker-version.txt"
    l1_scp_from /tmp/m80-jailer-version.txt "$WORK_ROOT/jailer-version.txt"
    python3 - "$local_proof" "$tag" "$REPO" "$run_status" "$remote_root" "$L1_SSH_TARGET" "$WORK_ROOT" <<'PY'
import json
import pathlib
import sys
from datetime import datetime, timezone

proof, tag, repo, status, remote_root, target, work = sys.argv[1:]
work = pathlib.Path(work)
stdout = (work / "run.stdout").read_text(errors="replace")
stderr = (work / "run.stderr").read_text(errors="replace")
after = (work / "firecracker-after.txt").read_text(errors="replace")
payload = {
    "schema_version": 1,
    "proof_kind": "nested-kvm-public-release-smoke",
    "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "release": {
        "repository": repo,
        "requested": tag,
        "resolved_tag": tag,
        "install_url": f"https://github.com/{repo}/releases/download/{tag}/install.sh",
    },
    "substrate": {
        "kind": "nested-kvm-l1",
        "ssh_target": target,
        "remote_root": remote_root,
        "dev_kvm_rw": True,
        "cpu_nested_flag_present": True,
        "firecracker_version": (work / "firecracker-version.txt").read_text(errors="replace").strip(),
        "jailer_version": (work / "jailer-version.txt").read_text(errors="replace").strip(),
    },
    "process_result": {
        "command": "M80_DEFAULT_PROFILE=env M80_ARTIFACT_DIR=<installed-artifacts> M80_KERNEL_IMAGE=<installed-kernel> M80_ROOTFS_IMAGE=<installed-rootfs> M80_RUN_ROOT=<remote-run-root> m80 run -- echo hello",
        "exit_status": int(status),
        "stdout": stdout,
        "stderr": stderr,
    },
    "install_status": json.loads((work / "install-status.json").read_text()),
    "cleanup": {
        "post_run_firecracker_processes": [line for line in after.splitlines() if line.strip()],
        "install_root_removed": (work / "install-root-removed.txt").read_text().strip() == "removed",
    },
}
pathlib.Path(proof).write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
PY
    python3 - "$local_proof" <<'PY'
import json
import pathlib
import sys
proof = json.loads(pathlib.Path(sys.argv[1]).read_text())
if proof["process_result"]["exit_status"] != 0:
    print(proof["process_result"]["stderr"], file=sys.stderr)
    raise SystemExit("level E process smoke failed")
if proof["process_result"]["stdout"] != "hello\n":
    raise SystemExit("level E stdout mismatch")
if proof["cleanup"]["post_run_firecracker_processes"]:
    raise SystemExit("level E cleanup leak: firecracker process still present")
if not proof["cleanup"]["install_root_removed"]:
    raise SystemExit("level E cleanup leak: install root remains")
print(f"nested-KVM release smoke proof: {sys.argv[1]}")
PY
}

cleanup() {
    local status=$?
    if [[ "$CLEANUP" -eq 1 && "$SPAWN_L1" -eq 1 ]]; then
        scripts/spawn-l1-runner.sh destroy --work-root "$SPAWNED_L1_WORK_ROOT" || true
    fi
    if [[ "$CLEANUP" -eq 1 && "$status" -eq 0 ]]; then
        rm -rf "$WORK_ROOT" 2>/dev/null || sudo -n rm -rf "$WORK_ROOT"
    else
        echo "validate-release: preserved work root $WORK_ROOT" >&2
    fi
}
trap cleanup EXIT

while [[ $# -gt 0 ]]; do
    case "$1" in
        --level)
            LEVEL="${2:?--level requires a value}"
            shift 2
            ;;
        --tag)
            TAG="${2:?--tag requires a value}"
            shift 2
            ;;
        --repo)
            REPO="${2:?--repo requires a value}"
            shift 2
            ;;
        --work-root)
            WORK_ROOT="${2:?--work-root requires a value}"
            shift 2
            ;;
        --dist-dir)
            DIST_DIR="${2:?--dist-dir requires a value}"
            shift 2
            ;;
        --skip-download)
            SKIP_DOWNLOAD=1
            shift
            ;;
        --proof-out)
            PROOF_OUT="${2:?--proof-out requires a value}"
            shift 2
            ;;
        --l1-ssh-target)
            L1_SSH_TARGET="${2:?--l1-ssh-target requires a value}"
            shift 2
            ;;
        --l1-ssh-key)
            L1_SSH_KEY="${2:?--l1-ssh-key requires a value}"
            shift 2
            ;;
        --l1-ssh-opts)
            L1_SSH_OPTS="${2:?--l1-ssh-opts requires a value}"
            shift 2
            ;;
        --l1-remote-base)
            L1_REMOTE_BASE="${2:?--l1-remote-base requires a value}"
            shift 2
            ;;
        --spawn-l1)
            SPAWN_L1=1
            shift
            ;;
        --keep-work)
            CLEANUP=0
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            die "unknown argument: $1"
            ;;
    esac
done

require_command python3
require_command jq
require_command sha256sum

mkdir -p "$WORK_ROOT"
RESOLVED_TAG="$(resolve_tag)"
DIST_DIR="${DIST_DIR:-$WORK_ROOT/dist}"

if [[ "$SPAWN_L1" -eq 1 ]]; then
    SPAWNED_L1_WORK_ROOT="$WORK_ROOT/l1"
    scripts/spawn-l1-runner.sh create --work-root "$SPAWNED_L1_WORK_ROOT" >"$WORK_ROOT/l1.env"
    # shellcheck disable=SC1090,SC1091
    source "$WORK_ROOT/l1.env"
    L1_SSH_TARGET="$M80_L1_SSH_TARGET"
    L1_SSH_KEY="$M80_L1_SSH_KEY"
    L1_SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=$SPAWNED_L1_WORK_ROOT/$M80_L1_NAME/known_hosts"
fi

case "$LEVEL" in
    a)
        level_a "$RESOLVED_TAG" "$DIST_DIR"
        ;;
    b)
        level_b
        ;;
    c)
        level_a "$RESOLVED_TAG" "$DIST_DIR"
        level_c "$RESOLVED_TAG" "$DIST_DIR"
        ;;
    d)
        level_a "$RESOLVED_TAG" "$DIST_DIR"
        level_d "$DIST_DIR"
        ;;
    e)
        level_e "$RESOLVED_TAG"
        ;;
    a-c)
        level_a "$RESOLVED_TAG" "$DIST_DIR"
        level_b
        level_c "$RESOLVED_TAG" "$DIST_DIR"
        ;;
    all)
        level_a "$RESOLVED_TAG" "$DIST_DIR"
        level_b
        level_c "$RESOLVED_TAG" "$DIST_DIR"
        level_d "$DIST_DIR"
        level_e "$RESOLVED_TAG"
        ;;
    *)
        die "unknown level: $LEVEL"
        ;;
esac

echo "validate-release: level=$LEVEL tag=$RESOLVED_TAG ok"
