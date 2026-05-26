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
  M80_E2E_TEST_TIMEOUT_SECONDS     default 180
  M80_KERNEL_IMAGE                 default /tmp/m80-build-current/artifacts/vmlinux
  M80_ROOTFS_IMAGE                 default /tmp/m80-build-current/artifacts/output.ext4
  M80_JAILER_HARDEN_BIN            default target/debug/m80-jailer-harden
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
jailer_harden_bin="${M80_JAILER_HARDEN_BIN:-$PWD/target/debug/m80-jailer-harden}"

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

have_harden=0
if [[ -x "$jailer_harden_bin" ]]; then
    have_harden=1
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
    python3 - "$dir/output.ext4.manifest.json" <<'PY'
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

minimal_artifacts=0
minimal_dir="${M80_MINIMAL_ARTIFACT_DIR:-/tmp/m80-build/minimal}"
if [[ -f "$minimal_dir/vmlinux" && -f "$minimal_dir/output.ext4" ]] && manifest_schema_ok "$minimal_dir"; then
    minimal_artifacts=1
fi

ubuntu_artifacts=0
ubuntu_dir="${M80_UBUNTU_ARTIFACT_DIR:-/tmp/m80-build/ubuntu}"
if [[ -f "$ubuntu_dir/vmlinux" && -f "$ubuntu_dir/output.ext4" ]] && manifest_schema_ok "$ubuntu_dir"; then
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
    return text[-4000:]

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
    "requires-pmem",
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
    if has_token requires-docker && [[ "$have_docker" -ne 1 ]]; then
        echo "requires-docker"
        return
    fi
    if has_token requires-pmem && [[ "$pmem_artifacts" -ne 1 ]]; then
        echo "missing-pmem-artifacts"
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
            "$kernel_image" "$rootfs_image" "$jailer_harden_bin" \
            "$list_only" "$timeout_s" "$have_sudo" "$have_kvm" "$have_artifacts" \
            "$have_harden" "$have_ip" "$have_cgroup_v2" "$have_loop_device" \
            "$have_debugfs" "$have_docker" "$measurement_enabled" \
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
    "schema_version": 1,
    "generated_at": datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z"),
    "package": sys.argv[3],
    "run_root": sys.argv[4],
    "list_only": flag(8),
    "timeout_seconds": int(sys.argv[9]),
    "environment": {
        "sudo_available": flag(10),
        "kvm_available": flag(11),
        "artifacts_available": flag(12),
        "jailer_harden_available": flag(13),
        "iproute2_available": flag(14),
        "cgroup_v2_available": flag(15),
        "loop_device_available": flag(16),
        "debugfs_available": flag(17),
        "docker_available": flag(18),
        "measurement_enabled": flag(19),
        "pmem_artifacts_available": flag(20),
        "external_network_enabled": flag(21),
        "malicious_artifacts_available": flag(22),
        "minimal_artifacts_available": flag(23),
        "ubuntu_artifacts_available": flag(24),
    },
    "artifacts": {
        "kernel": sys.argv[5],
        "rootfs": sys.argv[6],
        "jailer_harden": sys.argv[7],
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
cargo build -p m80-jailer-harden >/dev/null
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

        stdout_path="$tmpdir/${binary_name}.${test_name//[^A-Za-z0-9_.-]/_}.stdout"
        stderr_path="$tmpdir/${binary_name}.${test_name//[^A-Za-z0-9_.-]/_}.stderr"
        start_ns="$(date +%s%N)"
        exit_code=0
        run_env=(
            M80_KERNEL_IMAGE="$kernel_image"
            M80_ROOTFS_IMAGE="$rootfs_image"
            M80_JAILER_HARDEN_BIN="$jailer_harden_bin"
            M80_RUN_ROOT="$run_root"
            M80_RUN_EXTERNAL_NETWORK_E2E="${M80_RUN_EXTERNAL_NETWORK_E2E:-}"
        )
        if [[ -n "${M80_MALICIOUS_ARTIFACT_DIR:-}" ]]; then
            run_env+=(M80_MALICIOUS_ARTIFACT_DIR="$M80_MALICIOUS_ARTIFACT_DIR")
        fi
        if [[ -n "${M80_MINIMAL_ARTIFACT_DIR:-}" ]]; then
            run_env+=(M80_MINIMAL_ARTIFACT_DIR="$M80_MINIMAL_ARTIFACT_DIR")
        fi
        if [[ -n "${M80_UBUNTU_ARTIFACT_DIR:-}" ]]; then
            run_env+=(M80_UBUNTU_ARTIFACT_DIR="$M80_UBUNTU_ARTIFACT_DIR")
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
