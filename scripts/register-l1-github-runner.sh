#!/usr/bin/env bash
# Register an existing L1 VM as a one-job GitHub Actions runner.

set -Eeuo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPO="${M80_GITHUB_RUNNER_REPO:-moradology/m80}"
L1_NAME="${M80_L1_NAME:-m80-l1-runner}"
L1_WORK_ROOT="${M80_L1_WORK_ROOT:-/tank/tmp/m80-l1-runner}"
RUNNER_NAME="${M80_GITHUB_RUNNER_NAME:-}"
RUNNER_LABEL="${M80_GITHUB_RUNNER_LABEL:-}"
RUNNER_VERSION="${M80_GITHUB_RUNNER_VERSION:-latest}"
WAIT_SECONDS="${M80_GITHUB_RUNNER_WAIT_SECONDS:-120}"

usage() {
    cat <<'EOF'
Usage: scripts/register-l1-github-runner.sh [options]

Registers an already-created L1 VM as a GitHub Actions self-hosted runner using
config.sh --ephemeral, starts the runner process, and waits until GitHub reports
the runner online with the requested unique label.

Options:
  --repo OWNER/REPO       GitHub repository, default moradology/m80
  --l1-name NAME          L1 libvirt domain name, default m80-l1-runner
  --l1-work-root PATH     L1 state root, default /tank/tmp/m80-l1-runner
  --runner-name NAME      GitHub runner name, default same as --l1-name
  --runner-label LABEL    Unique label for this run, default same as runner name
  --runner-version VER    actions/runner version, default latest
  --wait-seconds N        Wait for runner to become online, default 120

The caller must be authenticated with gh as a repo admin because GitHub's
repository runner registration-token endpoint requires runner administration
authority.
EOF
}

die() {
    echo "register-l1-github-runner: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

shell_quote() {
    python3 - "$1" <<'PY'
import shlex
import sys
print(shlex.quote(sys.argv[1]))
PY
}

json_field() {
    local field="$1"
    python3 -c '
import json
import sys

payload = json.load(sys.stdin)
print(payload[sys.argv[1]])
' "$field"
}

runner_asset_url() {
    if [[ "$RUNNER_VERSION" == "latest" ]]; then
        gh api repos/actions/runner/releases/latest \
            --jq '[.assets[] | select(.name | test("^actions-runner-linux-x64-[0-9.]+\\.tar\\.gz$"))][0].browser_download_url'
        return 0
    fi
    local version="${RUNNER_VERSION#v}"
    printf 'https://github.com/actions/runner/releases/download/v%s/actions-runner-linux-x64-%s.tar.gz\n' \
        "$version" "$version"
}

validate_label() {
    local label="$1"
    if [[ ! "$label" =~ ^[A-Za-z0-9_.-]+$ ]]; then
        die "runner label must match [A-Za-z0-9_.-]+: $label"
    fi
}

wait_runner_online() {
    local deadline=$((SECONDS + WAIT_SECONDS))
    local payload found
    while (( SECONDS < deadline )); do
        payload="$(gh api "repos/$REPO/actions/runners")"
        if found="$(
            M80_RUNNER_NAME="$RUNNER_NAME" M80_RUNNER_LABEL="$RUNNER_LABEL" python3 -c '
import json
import os
import sys

runner_name = os.environ["M80_RUNNER_NAME"]
runner_label = os.environ["M80_RUNNER_LABEL"]
payload = json.load(sys.stdin)
for runner in payload.get("runners", []):
    labels = {label["name"] for label in runner.get("labels", [])}
    if (
        runner.get("name") == runner_name
        and runner.get("status") == "online"
        and runner_label in labels
        and "kvm" in labels
        and "self-hosted" in labels
    ):
        print(json.dumps({
            "id": runner.get("id"),
            "name": runner.get("name"),
            "status": runner.get("status"),
            "busy": runner.get("busy"),
            "labels": sorted(labels),
        }, sort_keys=True))
        sys.exit(0)
sys.exit(1)
' <<<"$payload"
        )"; then
            printf '%s\n' "$found"
            return 0
        fi
        sleep 3
    done
    die "timed out waiting for GitHub runner $RUNNER_NAME with label $RUNNER_LABEL"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --repo)
            REPO="${2:?--repo requires a value}"
            shift 2
            ;;
        --l1-name)
            L1_NAME="${2:?--l1-name requires a value}"
            shift 2
            ;;
        --l1-work-root)
            L1_WORK_ROOT="${2:?--l1-work-root requires a value}"
            shift 2
            ;;
        --runner-name)
            RUNNER_NAME="${2:?--runner-name requires a value}"
            shift 2
            ;;
        --runner-label)
            RUNNER_LABEL="${2:?--runner-label requires a value}"
            shift 2
            ;;
        --runner-version)
            RUNNER_VERSION="${2:?--runner-version requires a value}"
            shift 2
            ;;
        --wait-seconds)
            WAIT_SECONDS="${2:?--wait-seconds requires a value}"
            shift 2
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

RUNNER_NAME="${RUNNER_NAME:-$L1_NAME}"
RUNNER_LABEL="${RUNNER_LABEL:-$RUNNER_NAME}"
validate_label "$RUNNER_LABEL"
validate_label "$RUNNER_NAME"

require_command gh
require_command python3
require_command ssh

info_json="$(scripts/spawn-l1-runner.sh info --name "$L1_NAME" --work-root "$L1_WORK_ROOT")"
ssh_target="$(json_field ssh_target <<<"$info_json")"
ssh_key="$(json_field ssh_key <<<"$info_json")"
known_hosts="$L1_WORK_ROOT/$L1_NAME/known_hosts"
repo_url="https://github.com/$REPO"
registration_token="$(gh api -X POST "repos/$REPO/actions/runners/registration-token" --jq .token)"
asset_url="$(runner_asset_url)"
[[ -n "$registration_token" ]] || die "GitHub returned an empty registration token"
[[ -n "$asset_url" && "$asset_url" != "null" ]] || die "could not resolve actions/runner asset URL"

ssh_args=(
    ssh
    -i "$ssh_key"
    -o StrictHostKeyChecking=no
    -o UserKnownHostsFile="$known_hosts"
    -o ConnectTimeout=8
    "$ssh_target"
)

remote_command=$(
    printf 'REMOTE_REPO_URL=%s REMOTE_TOKEN=%s REMOTE_NAME=%s REMOTE_LABELS=%s REMOTE_ASSET_URL=%s bash -s' \
        "$(shell_quote "$repo_url")" \
        "$(shell_quote "$registration_token")" \
        "$(shell_quote "$RUNNER_NAME")" \
        "$(shell_quote "$RUNNER_LABEL,kvm")" \
        "$(shell_quote "$asset_url")"
)

"${ssh_args[@]}" "$remote_command" <<'REMOTE'
set -Eeuo pipefail

install_dir="${M80_ACTIONS_RUNNER_DIR:-$HOME/actions-runner}"
mkdir -p "$install_dir"
cd "$install_dir"

if [[ -f .runner ]]; then
    echo "actions runner is already configured in $install_dir; use a fresh L1 or remove it first" >&2
    exit 1
fi

if [[ ! -x ./config.sh ]]; then
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    curl -fL --connect-timeout 10 --max-time 300 --retry 2 --retry-delay 1 \
        -o "$tmp/actions-runner.tar.gz" "$REMOTE_ASSET_URL"
    tar -xzf "$tmp/actions-runner.tar.gz" -C "$install_dir"
fi

./config.sh \
    --url "$REMOTE_REPO_URL" \
    --token "$REMOTE_TOKEN" \
    --name "$REMOTE_NAME" \
    --labels "$REMOTE_LABELS" \
    --work "_work" \
    --unattended \
    --ephemeral \
    --replace

mkdir -p _diag
nohup bash -c 'cd "$1" && exec ./run.sh' _ "$install_dir" \
    >"_diag/m80-ephemeral-runner.log" 2>&1 </dev/null &
echo "$!" >"_diag/m80-ephemeral-runner.pid"
REMOTE

wait_runner_online
