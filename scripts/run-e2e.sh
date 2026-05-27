#!/usr/bin/env bash
# Run the privileged m80 real-KVM battery with structured pass/fail/skip output.
#
# This is intentionally a wrapper around Rust integration-test binaries, not a
# second test harness. Cargo still builds the tests and libtest still runs each
# test; this script adds environment detection, per-test skip reasons, a short
# default run-root, one-test-at-a-time execution, and a machine-readable report.

set -u -o pipefail

cd "$(dirname "$0")/.." || exit

json=0
list_only=0
package="m80-firecracker"
timeout_s="${M80_E2E_TEST_TIMEOUT_SECONDS:-180}"
run_root="${M80_E2E_RUN_ROOT:-/var/lib/m80-r}"

usage() {
    cat <<'EOF'
Usage: scripts/run-e2e.sh [--json] [--list] [--package <name>] [--timeout <seconds>]

Runs ignored privileged real-KVM integration tests one at a time and reports
pass/fail/skip with explicit reasons.

Environment:
  M80_E2E_RUN_ROOT                 default /var/lib/m80-r
  M80_E2E_REAP_MIN_AGE_HOURS       default 1
  M80_E2E_SKIP_REAPER=1            skip stale-state cleanup before a run
  M80_E2E_SKIP_LEAK_CHECK=1        skip post-test cleanup verification
  M80_E2E_TEST_TIMEOUT_SECONDS     default 180
  M80_ARTIFACT_DIR                 default dirname of M80_ROOTFS_IMAGE
  M80_KERNEL_IMAGE                 default /tmp/m80-build-current/artifacts/vmlinux
  M80_ROOTFS_IMAGE                 default /tmp/m80-build-current/artifacts/output.ext4
  M80_FIRECRACKER_SECCOMP_FILTER   default /opt/firecracker/bin/firecracker-seccomp-filter.bin
  M80_JAILER_HARDEN_BIN            default target/debug/m80-jailer-harden
  M80_NET_HELPER_BIN               default target/debug/m80-net-helper
  M80_JAIL_UID / M80_JAIL_GID       override test jail identity when 3000 is unavailable
  M80_RUN_EXTERNAL_NETWORK_E2E=1   opt into external-network egress tests
  M80_MALICIOUS_ARTIFACT_DIR       enables malicious-guestd real-KVM tests
  M80_MINIMAL_ARTIFACT_DIR         enables image-kind Minimal tests
  M80_UBUNTU_ARTIFACT_DIR          enables image-kind Ubuntu tests
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --json)
            json=1
            shift
            ;;
        --list)
            list_only=1
            shift
            ;;
        --package|-p)
            package="${2:?--package requires a value}"
            shift 2
            ;;
        --timeout)
            timeout_s="${2:?--timeout requires a value}"
            shift 2
            ;;
        --help|-h)
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

kernel_image="${M80_KERNEL_IMAGE:-/tmp/m80-build-current/artifacts/vmlinux}"
rootfs_image="${M80_ROOTFS_IMAGE:-/tmp/m80-build-current/artifacts/output.ext4}"
artifact_dir="${M80_ARTIFACT_DIR:-$(dirname "$rootfs_image")}"
firecracker_seccomp_filter="${M80_FIRECRACKER_SECCOMP_FILTER:-/opt/firecracker/bin/firecracker-seccomp-filter.bin}"
host_binaries_manifest="$artifact_dir/host-binaries.manifest.json"
absolute_repo_path() {
    case "$1" in
        /*) printf '%s\n' "$1" ;;
        *) printf '%s/%s\n' "$PWD" "$1" ;;
    esac
}
jailer_harden_bin="$(absolute_repo_path "${M80_JAILER_HARDEN_BIN:-target/debug/m80-jailer-harden}")"
net_helper_bin="$(absolute_repo_path "${M80_NET_HELPER_BIN:-target/debug/m80-net-helper}")"

sudo_cmd=()
if [[ "$(id -u)" -ne 0 ]]; then
    sudo_cmd=(sudo -n)
fi

have_sudo=1
if [[ "${#sudo_cmd[@]}" -gt 0 ]] && ! sudo -n true >/dev/null 2>&1; then
    have_sudo=0
fi

have_kvm=0
if [[ -r /dev/kvm && -w /dev/kvm ]]; then
    have_kvm=1
fi

have_artifacts=0
if [[ -f "$kernel_image" && -f "$rootfs_image" ]]; then
    have_artifacts=1
fi

have_seccomp_filter=0
if [[ -f "$firecracker_seccomp_filter" ]]; then
    have_seccomp_filter=1
fi

have_host_manifest=0
if [[ -f "$host_binaries_manifest" ]]; then
    have_host_manifest=1
fi

have_harden=0
if [[ -x "$jailer_harden_bin" ]]; then
    have_harden=1
fi

have_net_helper=0
if [[ -x "$net_helper_bin" ]]; then
    have_net_helper=1
fi

have_ip=0
if command -v ip >/dev/null 2>&1; then
    have_ip=1
fi

have_cgroup_v2=0
if [[ -f /sys/fs/cgroup/cgroup.controllers ]]; then
    have_cgroup_v2=1
fi

have_loop_device=0
if [[ -e /dev/loop-control || -e /dev/loop0 ]]; then
    have_loop_device=1
fi

have_debugfs=0
if command -v debugfs >/dev/null 2>&1; then
    have_debugfs=1
fi

have_mkfs_erofs=0
if command -v mkfs.erofs >/dev/null 2>&1; then
    have_mkfs_erofs=1
fi

have_systemd=0
if command -v systemd-run >/dev/null 2>&1 && [[ -d /run/systemd/system ]]; then
    have_systemd=1
fi

have_docker=0
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    have_docker=1
fi

measurement_enabled=0
if [[ "${M80_RUN_MEASUREMENT_E2E:-}" == "1" ]]; then
    measurement_enabled=1
fi

pmem_artifacts=0
if [[ -n "${M80_PMEM_LAYERS:-}" || -n "${M80_PMEM_EROFS_IMAGE:-}" || -n "${M80_PMEM_LAYER_ARTIFACT_DIR:-}" ]]; then
    pmem_artifacts=1
fi

external_network_enabled=0
if [[ "${M80_RUN_EXTERNAL_NETWORK_E2E:-}" == "1" ]]; then
    external_network_enabled=1
fi

malicious_artifacts=0
if [[ -n "${M80_MALICIOUS_ARTIFACT_DIR:-}" && -d "${M80_MALICIOUS_ARTIFACT_DIR:-}" ]]; then
    malicious_artifacts=1
fi

manifest_schema_ok() {
    local dir="$1"
    local expected_kind="$2"
    python3 - "$dir/output.ext4.manifest.json" "$expected_kind" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
expected_kind = sys.argv[2]
if not path.exists():
    raise SystemExit(1)
try:
    data = json.loads(path.read_text())
except Exception:
    raise SystemExit(1)
raise SystemExit(
    0
    if data.get("schema_version") == 5
    and data.get("image_kind") == expected_kind
    else 1
)
PY
}

image_kind_artifacts_available() {
    local dir="$1"
    local expected_kind="$2"
    [[ -f "$dir/vmlinux" ]] \
        && [[ -f "$dir/output.ext4" ]] \
        && [[ -f "$dir/host-binaries.manifest.json" ]] \
        && manifest_schema_ok "$dir" "$expected_kind"
}

minimal_artifacts=0
minimal_dir="${M80_MINIMAL_ARTIFACT_DIR:-$artifact_dir}"
if image_kind_artifacts_available "$minimal_dir" "minimal"; then
    minimal_artifacts=1
fi

ubuntu_artifacts=0
ubuntu_dir="${M80_UBUNTU_ARTIFACT_DIR:-/tmp/m80-build/ubuntu}"
if image_kind_artifacts_available "$ubuntu_dir" "ubuntu"; then
    ubuntu_artifacts=1
fi

tmpdir="$(mktemp -d /tmp/m80-e2e-report.XXXXXX)"
trap 'rm -rf "$tmpdir"' EXIT
build_json="$tmpdir/cargo-test-artifacts.jsonl"
executables="$tmpdir/executables.txt"
results="$tmpdir/results.jsonl"
ignore_reasons="$tmpdir/ignore-reasons.tsv"

record_result() {
    local status="$1"
    local test_name="$2"
    local binary_name="$3"
    local reason="$4"
    local exit_code="$5"
    local duration_ms="$6"
    local stdout_path="$7"
    local stderr_path="$8"
    RESULT_STATUS="$status" \
    RESULT_TEST="$test_name" \
    RESULT_BINARY="$binary_name" \
    RESULT_REASON="$reason" \
    RESULT_EXIT_CODE="$exit_code" \
    RESULT_DURATION_MS="$duration_ms" \
    RESULT_STDOUT="$stdout_path" \
    RESULT_STDERR="$stderr_path" \
    python3 - "$results" <<'PY'
import json
import os
import pathlib
import sys

def excerpt(path):
    if not path:
        return ""
    p = pathlib.Path(path)
    if not p.exists():
        return ""
    text = p.read_text(errors="replace")
    if len(text) <= 6000:
        return text
    return text[:2000] + "\n... <m80 excerpt truncated> ...\n" + text[-4000:]

entry = {
    "status": os.environ["RESULT_STATUS"],
    "test": os.environ["RESULT_TEST"],
    "binary": os.environ["RESULT_BINARY"],
    "reason": os.environ["RESULT_REASON"] or None,
    "exit_code": int(os.environ["RESULT_EXIT_CODE"]),
    "duration_ms": int(os.environ["RESULT_DURATION_MS"]),
}
if entry["status"] == "fail":
    entry["stdout_excerpt"] = excerpt(os.environ["RESULT_STDOUT"])
    entry["stderr_excerpt"] = excerpt(os.environ["RESULT_STDERR"])
with open(sys.argv[1], "a", encoding="utf-8") as f:
    f.write(json.dumps(entry, sort_keys=True) + "\n")
PY
}

cleanup_after_leak() {
    if [[ "${M80_E2E_SKIP_REAPER:-0}" == "1" || "$have_sudo" -ne 1 ]]; then
        return
    fi
    scripts/e2e-reap.sh \
        --run-root "$run_root" \
        --min-age-hours 0 \
        >/dev/null 2>&1 || true
}

write_ignore_reason_map() {
    local package_name="$1"
    local out_path="$2"
    python3 - "$package_name" "$out_path" <<'PY'
import pathlib
import re
import sys

package = sys.argv[1]
out = pathlib.Path(sys.argv[2])
crate = pathlib.Path("crates") / package
allowed = {
    "requires-kvm",
    "requires-root",
    "requires-network-namespace",
    "slow",
    "requires-cgroup-v2",
    "requires-artifacts",
    "requires-external-network",
    "requires-malicious-artifacts",
    "requires-docker",
    "requires-mount-namespace",
    "requires-loop-device",
    "requires-snapshot-support",
    "requires-debugfs",
    "requires-erofs-tool",
    "requires-pmem",
    "requires-systemd",
    "measurement",
    "manual",
}
pattern = re.compile(
    r'(?ms)^[ \t]*#\s*\[\s*ignore\s*=\s*"([^"]+)"\s*\]\s*'
    r'(?:^[ \t]*#\[[^\n]*\]\s*)*'
    r'^[ \t]*(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)'
)

entries = {}
if crate.exists():
    for path in sorted(crate.rglob("*.rs")):
        text = path.read_text()
        for match in pattern.finditer(text):
            reason = match.group(1)
            tokens = reason.split()
            unknown = [token for token in tokens if token not in allowed]
            if not tokens or unknown:
                location = f"{path}:{text.count(chr(10), 0, match.start()) + 1}"
                raise SystemExit(f"{location}: invalid #[ignore] reason {reason!r}")
            entries[match.group(2)] = reason

with out.open("w", encoding="utf-8") as fh:
    for name, reason in sorted(entries.items()):
        fh.write(f"{name}\t{reason}\n")
PY
}

ignore_tokens_for() {
    local test_name="$1"
    awk -F '\t' -v test="$test_name" '
        test == $1 || test ~ ("(^|::)" $1 "$") { print $2; found = 1; exit }
        END { if (!found) print "" }
    ' "$ignore_reasons"
}

skip_reason_for() {
    local binary_name="$1"
    local test_name="$2"
    local tokens
    tokens="$(ignore_tokens_for "$test_name")"

    has_token() {
        local token="$1"
        [[ " $tokens " == *" $token "* ]]
    }

    if [[ -z "$tokens" ]]; then
        echo "unclassified-ignore"
        return
    fi
    if has_token manual; then
        echo "manual-ignore"
        return
    fi
    if has_token measurement && [[ "$measurement_enabled" -ne 1 ]]; then
        echo "measurement-not-enabled"
        return
    fi
    if has_token requires-root && [[ "$have_sudo" -ne 1 ]]; then
        echo "sudo-not-available"
        return
    fi
    if has_token requires-kvm && [[ "$have_kvm" -ne 1 ]]; then
        echo "requires-kvm"
        return
    fi
    if has_token requires-artifacts && [[ "$have_artifacts" -ne 1 ]]; then
        echo "missing-kernel-or-rootfs-artifacts"
        return
    fi
    if has_token requires-kvm && [[ "$have_harden" -ne 1 ]]; then
        echo "missing-m80-jailer-harden"
        return
    fi
    if has_token requires-kvm && [[ "$have_net_helper" -ne 1 ]]; then
        echo "missing-m80-net-helper"
        return
    fi
    if has_token requires-kvm && [[ "$have_seccomp_filter" -ne 1 ]]; then
        echo "missing-firecracker-seccomp-filter"
        return
    fi
    if has_token requires-kvm && [[ "$have_host_manifest" -ne 1 ]]; then
        echo "missing-host-binaries-manifest"
        return
    fi
    if has_token requires-network-namespace && [[ "$have_ip" -ne 1 ]]; then
        echo "missing-iproute2"
        return
    fi
    if has_token requires-cgroup-v2 && [[ "$have_cgroup_v2" -ne 1 ]]; then
        echo "requires-cgroup-v2"
        return
    fi
    if has_token requires-loop-device && [[ "$have_loop_device" -ne 1 ]]; then
        echo "missing-loop-device"
        return
    fi
    if has_token requires-debugfs && [[ "$have_debugfs" -ne 1 ]]; then
        echo "missing-debugfs"
        return
    fi
    if has_token requires-erofs-tool && [[ "$have_mkfs_erofs" -ne 1 ]]; then
        echo "missing-mkfs-erofs"
        return
    fi
    if has_token requires-docker && [[ "$have_docker" -ne 1 ]]; then
        echo "requires-docker"
        return
    fi
    if has_token requires-pmem && [[ "$pmem_artifacts" -ne 1 ]]; then
        echo "missing-pmem-artifacts"
        return
    fi
    if has_token requires-systemd && [[ "$have_systemd" -ne 1 ]]; then
        echo "requires-systemd"
        return
    fi
    if has_token requires-external-network && [[ "$external_network_enabled" -ne 1 ]]; then
        echo "external-network-not-enabled"
        return
    fi
    if has_token requires-malicious-artifacts && [[ "$malicious_artifacts" -ne 1 ]]; then
        echo "missing-malicious-artifacts"
        return
    fi
    if [[ "$binary_name" == *image_kind_dispatch_real_kvm* ]]; then
        case "$test_name" in
            *minimal*|*overlay_assembly*)
                if [[ "$minimal_artifacts" -ne 1 ]]; then
                    echo "missing-minimal-artifacts"
                    return
                fi
                ;;
        esac
        case "$test_name" in
            *ubuntu*|*overlay_assembly*)
                if [[ "$ubuntu_artifacts" -ne 1 ]]; then
                    echo "missing-ubuntu-artifacts"
                    return
                fi
                ;;
        esac
    fi
    echo ""
}

emit_report() {
    local exit_code="$1"
    if [[ "$json" -eq 1 ]]; then
        python3 - \
            "$results" "$exit_code" "$package" "$run_root" \
            "$artifact_dir" "$kernel_image" "$rootfs_image" "$firecracker_seccomp_filter" \
            "$host_binaries_manifest" "$jailer_harden_bin" "$net_helper_bin" \
            "$list_only" "$timeout_s" "$have_sudo" "$have_kvm" "$have_artifacts" \
            "$have_seccomp_filter" "$have_host_manifest" "$have_harden" "$have_net_helper" \
            "$have_ip" "$have_cgroup_v2" "$have_loop_device" \
            "$have_debugfs" "$have_mkfs_erofs" "$have_docker" "$measurement_enabled" \
            "$pmem_artifacts" "$external_network_enabled" "$malicious_artifacts" \
            "$minimal_artifacts" "$ubuntu_artifacts" <<'PY'
import datetime
import json
import pathlib
import sys
from collections import Counter

def flag(index):
    return bool(int(sys.argv[index]))

path = pathlib.Path(sys.argv[1])
results = []
if path.exists():
    results = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
counts = Counter(item["status"] for item in results)
report = {
    "schema_version": 3,
    "generated_at": datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z"),
    "package": sys.argv[3],
    "run_root": sys.argv[4],
    "list_only": flag(12),
    "timeout_seconds": int(sys.argv[13]),
    "environment": {
        "sudo_available": flag(14),
        "kvm_available": flag(15),
        "artifacts_available": flag(16),
        "firecracker_seccomp_filter_available": flag(17),
        "host_binaries_manifest_available": flag(18),
        "jailer_harden_available": flag(19),
        "net_helper_available": flag(20),
        "iproute2_available": flag(21),
        "cgroup_v2_available": flag(22),
        "loop_device_available": flag(23),
        "debugfs_available": flag(24),
        "mkfs_erofs_available": flag(25),
        "docker_available": flag(26),
        "measurement_enabled": flag(27),
        "pmem_artifacts_available": flag(28),
        "external_network_enabled": flag(29),
        "malicious_artifacts_available": flag(30),
        "minimal_artifacts_available": flag(31),
        "ubuntu_artifacts_available": flag(32),
    },
    "artifacts": {
        "artifact_dir": sys.argv[5],
        "kernel": sys.argv[6],
        "rootfs": sys.argv[7],
        "firecracker_seccomp_filter": sys.argv[8],
        "host_binaries_manifest": sys.argv[9],
        "jailer_harden": sys.argv[10],
        "net_helper": sys.argv[11],
    },
    "summary": {
        "passed": counts["pass"],
        "failed": counts["fail"],
        "skipped": counts["skip"],
        "listed": counts["list"],
        "total": len(results),
    },
    "results": results,
    "exit_code": int(sys.argv[2]),
}
print(json.dumps(report, indent=2, sort_keys=True))
PY
    else
        python3 - "$results" "$exit_code" <<'PY'
import json
import pathlib
import sys
from collections import Counter

path = pathlib.Path(sys.argv[1])
results = []
if path.exists():
    results = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
counts = Counter(item["status"] for item in results)
print(f"summary: pass={counts['pass']} fail={counts['fail']} skip={counts['skip']} list={counts['list']} total={len(results)}")
for item in results:
    reason = f" ({item['reason']})" if item.get("reason") else ""
    print(f"{item['status']:>4} {item['binary']}::{item['test']}{reason} [{item['duration_ms']}ms]")
print(f"exit_code={sys.argv[2]}")
PY
    fi
}

if [[ "$list_only" -eq 0 ]]; then
    if [[ "${M80_E2E_SKIP_REAPER:-0}" != "1" && "$have_sudo" -eq 1 ]]; then
        if ! scripts/e2e-reap.sh \
            --run-root "$run_root" \
            --min-age-hours "${M80_E2E_REAP_MIN_AGE_HOURS:-1}" \
            >/dev/null; then
            echo "e2e reaper failed before test execution" >&2
            exit 1
        fi
    fi
    "${sudo_cmd[@]}" mkdir -p "$run_root"
    if [[ "${#sudo_cmd[@]}" -gt 0 ]]; then
        "${sudo_cmd[@]}" chown "$(id -u):$(id -g)" "$run_root" || true
    fi
fi

write_ignore_reason_map "$package" "$ignore_reasons"
cargo build -p m80-jailer-harden --features no-systemd-launch >/dev/null
cargo test -p "$package" --tests --no-run --message-format=json > "$build_json"

python3 - "$build_json" > "$executables" <<'PY'
import json
import pathlib
import sys

seen = set()
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    if not line.strip():
        continue
    event = json.loads(line)
    if event.get("reason") != "compiler-artifact":
        continue
    target = event.get("target") or {}
    if not target.get("test"):
        continue
    exe = event.get("executable")
    if not exe or exe in seen:
        continue
    seen.add(exe)
    print(exe)
PY

while IFS= read -r exe; do
    [[ -n "$exe" && -x "$exe" ]] || continue
    binary_name="$(basename "$exe")"
    while IFS= read -r line; do
        [[ "$line" == *": test" ]] || continue
        test_name="${line%: test}"
        skip_reason="$(skip_reason_for "$binary_name" "$test_name")"
        if [[ -n "$skip_reason" ]]; then
            record_result "skip" "$test_name" "$binary_name" "$skip_reason" 0 0 "" ""
            continue
        fi
        if [[ "$list_only" -eq 1 ]]; then
            record_result "list" "$test_name" "$binary_name" "" 0 0 "" ""
            continue
        fi

        leak_before_path=""
        leak_before_stderr_path=""
        if [[ "${M80_E2E_SKIP_LEAK_CHECK:-0}" != "1" ]]; then
            leak_before_path="$tmpdir/${binary_name}.${test_name//[^A-Za-z0-9_.-]/_}.leak.before.json"
            leak_before_stderr_path="$tmpdir/${binary_name}.${test_name//[^A-Za-z0-9_.-]/_}.leak.before.stderr"
            leak_status=0
            scripts/e2e-reap.sh \
                --dry-run \
                --json \
                --run-root "$run_root" \
                --min-age-hours 0 \
                >"$leak_before_path" 2>"$leak_before_stderr_path" || leak_status=$?
            if [[ "$leak_status" -ne 0 ]]; then
                record_result "fail" "$test_name" "$binary_name" "leak-check-error" 90 0 "$leak_before_path" "$leak_before_stderr_path"
                continue
            fi
        fi

        stdout_path="$tmpdir/${binary_name}.${test_name//[^A-Za-z0-9_.-]/_}.stdout"
        stderr_path="$tmpdir/${binary_name}.${test_name//[^A-Za-z0-9_.-]/_}.stderr"
        start_ns="$(date +%s%N)"
        exit_code=0
        run_env=(
            M80_ARTIFACT_DIR="$artifact_dir"
            M80_KERNEL_IMAGE="$kernel_image"
            M80_ROOTFS_IMAGE="$rootfs_image"
            M80_FIRECRACKER_SECCOMP_FILTER="$firecracker_seccomp_filter"
            M80_JAILER_HARDEN_BIN="$jailer_harden_bin"
            M80_NET_HELPER_BIN="$net_helper_bin"
            M80_RUN_ROOT="$run_root"
            M80_RUN_EXTERNAL_NETWORK_E2E="${M80_RUN_EXTERNAL_NETWORK_E2E:-}"
            M80_MINIMAL_ARTIFACT_DIR="$minimal_dir"
            M80_UBUNTU_ARTIFACT_DIR="$ubuntu_dir"
        )
        if [[ -n "${M80_MALICIOUS_ARTIFACT_DIR:-}" ]]; then
            run_env+=(M80_MALICIOUS_ARTIFACT_DIR="$M80_MALICIOUS_ARTIFACT_DIR")
        fi
        if [[ -n "${M80_FIRECRACKER_BIN:-}" ]]; then
            run_env+=(M80_FIRECRACKER_BIN="$M80_FIRECRACKER_BIN")
        fi
        if [[ -n "${M80_JAILER_BIN:-}" ]]; then
            run_env+=(M80_JAILER_BIN="$M80_JAILER_BIN")
        fi
        if [[ -n "${M80_JAIL_UID:-}" ]]; then
            run_env+=(M80_JAIL_UID="$M80_JAIL_UID")
        fi
        if [[ -n "${M80_JAIL_GID:-}" ]]; then
            run_env+=(M80_JAIL_GID="$M80_JAIL_GID")
        fi
        timeout "$timeout_s" "${sudo_cmd[@]}" env "${run_env[@]}" \
            "$exe" --ignored --exact "$test_name" --nocapture --test-threads=1 \
            >"$stdout_path" 2>"$stderr_path" || exit_code=$?
        end_ns="$(date +%s%N)"
        duration_ms=$(( (end_ns - start_ns) / 1000000 ))
        if [[ "$exit_code" -eq 0 ]]; then
            if [[ "${M80_E2E_SKIP_LEAK_CHECK:-0}" != "1" ]]; then
                leak_after_path="$tmpdir/${binary_name}.${test_name//[^A-Za-z0-9_.-]/_}.leak.after.json"
                leak_stderr_path="$tmpdir/${binary_name}.${test_name//[^A-Za-z0-9_.-]/_}.leak.after.stderr"
                leak_diff_path="$tmpdir/${binary_name}.${test_name//[^A-Za-z0-9_.-]/_}.leak.diff.json"
                leak_diff_stderr_path="$tmpdir/${binary_name}.${test_name//[^A-Za-z0-9_.-]/_}.leak.diff.stderr"
                leak_status=0
                scripts/e2e-reap.sh \
                    --dry-run \
                    --json \
                    --run-root "$run_root" \
                    --min-age-hours 0 \
                    >"$leak_after_path" 2>"$leak_stderr_path" || leak_status=$?
                if [[ "$leak_status" -ne 0 ]]; then
                    record_result "fail" "$test_name" "$binary_name" "leak-check-error" 90 "$duration_ms" "$leak_after_path" "$leak_stderr_path"
                    cleanup_after_leak
                    continue
                fi
                leak_diff_status=0
                python3 scripts/e2e-leak-diff.py \
                    "$leak_before_path" \
                    "$leak_after_path" \
                    "$leak_diff_path" \
                    2>"$leak_diff_stderr_path" || leak_diff_status=$?
                if [[ "$leak_diff_status" -eq 1 ]]; then
                    record_result "fail" "$test_name" "$binary_name" "leak-check" 91 "$duration_ms" "$leak_diff_path" "$leak_diff_stderr_path"
                    cleanup_after_leak
                    continue
                fi
                if [[ "$leak_diff_status" -ne 0 ]]; then
                    record_result "fail" "$test_name" "$binary_name" "leak-check-error" 90 "$duration_ms" "$leak_diff_path" "$leak_diff_stderr_path"
                    cleanup_after_leak
                    continue
                fi
            fi
            record_result "pass" "$test_name" "$binary_name" "" "$exit_code" "$duration_ms" "$stdout_path" "$stderr_path"
        elif [[ "$exit_code" -eq 124 ]]; then
            record_result "fail" "$test_name" "$binary_name" "timeout" "$exit_code" "$duration_ms" "$stdout_path" "$stderr_path"
        else
            record_result "fail" "$test_name" "$binary_name" "" "$exit_code" "$duration_ms" "$stdout_path" "$stderr_path"
        fi
    done < <("$exe" --ignored --list)
done < "$executables"

failures="$(python3 - "$results" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
count = 0
if path.exists():
    for line in path.read_text().splitlines():
        if line.strip() and json.loads(line)["status"] == "fail":
            count += 1
print(count)
PY
)"

final_exit=0
if [[ "$failures" -gt 0 ]]; then
    final_exit=1
fi
emit_report "$final_exit"
exit "$final_exit"
