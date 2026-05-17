#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

mapfile -t firecracker_pids < <(pgrep -x firecracker || true)

if ((${#firecracker_pids[@]} == 0)); then
    echo "quiet-host preflight: no firecracker processes found"
    exit 0
fi

echo "quiet-host preflight: found firecracker processes"
echo
ps -eo pid,ppid,user,comm,args | grep -E 'firecracker|sandbox-executor-rs' | grep -v grep || true

show_kubernetes_owner() {
    local pod_uid=$1

    if ! command -v kubectl >/dev/null 2>&1; then
        echo "kubectl not found; pod uid from cgroup: $pod_uid"
        return
    fi

    if ! command -v jq >/dev/null 2>&1; then
        echo "jq not found; inspect pod uid with:"
        echo "  kubectl get pod -A -o json | grep -C 20 '$pod_uid'"
        return
    fi

    kubectl get pod -A -o json |
        jq --arg uid "$pod_uid" -r '
          .items[]
          | select(.metadata.uid == $uid)
          | {
              namespace: .metadata.namespace,
              name: .metadata.name,
              uid: .metadata.uid,
              node: .spec.nodeName,
              phase: .status.phase,
              ownerReferences: .metadata.ownerReferences,
              containerStatuses: [
                .status.containerStatuses[]
                | {
                    name,
                    image,
                    ready,
                    restartCount,
                    containerID,
                    state
                  }
              ]
            }
          | @json'
}

show_user_scope() {
    local scope=$1

    if command -v systemctl >/dev/null 2>&1; then
        systemctl --user status "$scope" --no-pager --lines=0 2>/dev/null | sed -n '1,80p' || true
    else
        echo "systemctl not found; user scope from cgroup: $scope"
    fi
}

declare -A seen_pod_uids=()
declare -A seen_user_scopes=()

for pid in "${firecracker_pids[@]}"; do
    echo
    echo "== firecracker pid $pid =="
    ps -p "$pid" -o pid=,ppid=,user=,comm=,args= || true

    printf "cwd: "
    readlink "/proc/$pid/cwd" 2>/dev/null || echo "(unreadable)"

    if [[ ! -r "/proc/$pid/cgroup" ]]; then
        echo "cgroup: (unreadable)"
        continue
    fi

    cgroup=$(sed -n '1p' "/proc/$pid/cgroup")
    echo "cgroup: $cgroup"

    if [[ "$cgroup" =~ pod([0-9a-f_]+)\.slice ]]; then
        pod_uid=${BASH_REMATCH[1]//_/-}
        echo "kubernetes pod uid: $pod_uid"
        if [[ -z "${seen_pod_uids[$pod_uid]+x}" ]]; then
            seen_pod_uids[$pod_uid]=1
            show_kubernetes_owner "$pod_uid"
        else
            echo "kubernetes owner already shown for pod uid: $pod_uid"
        fi
    elif [[ "$cgroup" == *"/user.slice/"* && "$cgroup" =~ /([^/]+\.scope)$ ]]; then
        scope=${BASH_REMATCH[1]}
        echo "user scope: $scope"
        if [[ -z "${seen_user_scopes[$scope]+x}" ]]; then
            seen_user_scopes[$scope]=1
            show_user_scope "$scope"
        else
            echo "user scope owner already shown: $scope"
        fi
    fi
done

echo
echo "Refusing close-quality q420k measurement on this host until the listed processes are gone."
echo "This script is inventory only; it does not drain, signal, or kill anything."
exit 1
