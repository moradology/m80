#!/usr/bin/env bash
# CLI facade smoke path for WRA0.
#
# Default mode is non-KVM and safe for ordinary CI:
#   ./scripts/smoke-cli.sh
#
# KVM mode requires a Linux host with Firecracker, jailer, m80 kernel/rootfs
# artifacts, and enough privilege to create the sandbox:
#   sudo M80_RUN_KVM_E2E=1 \
#        M80_FIRECRACKER_BIN=/path/to/firecracker \
#        M80_JAILER_BIN=/path/to/jailer \
#        M80_KERNEL_IMAGE=/path/to/vmlinux \
#        M80_ROOTFS_IMAGE=/path/to/rootfs.ext4 \
#        ./scripts/smoke-cli.sh

set -euo pipefail

cd "$(dirname "$0")/.."

RUN_KVM="${M80_RUN_KVM_E2E:-0}"
RUN_ROOT="${M80_RUN_ROOT:-$(mktemp -d /tmp/m80-smoke-cli.XXXXXX)}"
M80_BIN="${M80_BIN:-target/debug/m80}"
OUT_DIR="$(mktemp -d /tmp/m80-smoke-cli-output.XXXXXX)"
KEEP_OUTPUT="${M80_SMOKE_KEEP_OUTPUT:-0}"

cleanup() {
    if [[ "$KEEP_OUTPUT" != "1" ]]; then
        rm -rf "$OUT_DIR"
    else
        echo "kept smoke output: $OUT_DIR" >&2
    fi
}
trap cleanup EXIT

dump_triage() {
    local code="$1"
    echo "=== SMOKE FAILED: $code ===" >&2
    echo "run_root=$RUN_ROOT" >&2
    if [[ -d "$RUN_ROOT" ]]; then
        find "$RUN_ROOT" -maxdepth 3 -type f \
            \( -name state.json -o -name diagnostics.jsonl -o -name console.log -o -name owner.json \) \
            -print >&2 || true
        while IFS= read -r file; do
            echo "--- $file ---" >&2
            tail -n 80 "$file" >&2 || true
        done < <(find "$RUN_ROOT" -maxdepth 3 -type f \
            \( -name diagnostics.jsonl -o -name console.log -o -name owner.json \) 2>/dev/null)
    fi
}

fail() {
    dump_triage "$1"
    exit 1
}

need_env() {
    local name="$1"
    if [[ -z "${!name:-}" ]]; then
        fail "missing required env $name"
    fi
}

run_cmd() {
    local label="$1"
    shift
    echo "=== $label ==="
    "$@" >"$OUT_DIR/$label.stdout" 2>"$OUT_DIR/$label.stderr" || {
        cat "$OUT_DIR/$label.stdout"
        cat "$OUT_DIR/$label.stderr" >&2
        fail "$label"
    }
}

expect_exit() {
    local label="$1"
    local expected="$2"
    shift 2
    echo "=== $label ==="
    set +e
    "$@" >"$OUT_DIR/$label.stdout" 2>"$OUT_DIR/$label.stderr"
    local status=$?
    set -e
    if [[ "$status" != "$expected" ]]; then
        cat "$OUT_DIR/$label.stdout"
        cat "$OUT_DIR/$label.stderr" >&2
        fail "$label expected exit $expected got $status"
    fi
}

build_cli() {
    run_cmd build-cli cargo build -p m80-cli
}

run_non_kvm_ci() {
    run_cmd cli-tests cargo test -p m80-cli --tests
}

configure_kvm_env() {
    need_env M80_FIRECRACKER_BIN
    need_env M80_JAILER_BIN
    need_env M80_KERNEL_IMAGE
    need_env M80_ROOTFS_IMAGE
    mkdir -p "$RUN_ROOT"
    export HOME="${HOME:-$OUT_DIR/home}"
    mkdir -p "$HOME"
    export M80_RUN_ROOT="$RUN_ROOT"
    export M80_CGROUP_MODE="${M80_CGROUP_MODE:-disabled}"
    export M80_MAX_CONCURRENT_VMS="${M80_MAX_CONCURRENT_VMS:-4}"
}

m80() {
    "$M80_BIN" "$@"
}

run_kvm_smoke() {
    configure_kvm_env
    run_cmd version m80 version
    run_cmd help-top m80 --help
    run_cmd help-run m80 run --help
    run_cmd help-warm m80 warm --help
    run_cmd preflight m80 preflight
    run_cmd config-show m80 config show

    run_cmd run-profile-env m80 run --profile env --egress none -- /bin/true
    run_cmd example-echo-hello sh examples/echo-hello/run.sh

    echo "=== stdout-stderr probe ==="
    m80 run --egress none -- /bin/sh -c 'printf smoke-out; printf smoke-err >&2' \
        >"$OUT_DIR/stdio.stdout" 2>"$OUT_DIR/stdio.stderr" || fail stdout-stderr-probe
    [[ "$(cat "$OUT_DIR/stdio.stdout")" == "smoke-out" ]] || fail stdout-stderr-stdout
    [[ "$(cat "$OUT_DIR/stdio.stderr")" == "smoke-err" ]] || fail stdout-stderr-stderr

    run_workspace_probe
    expect_exit failing-child 17 \
        m80 run --egress none -- /bin/sh -c 'printf fail-out; printf fail-err >&2; exit 17'
    [[ "$(cat "$OUT_DIR/failing-child.stdout")" == "fail-out" ]] || fail failing-child-stdout
    [[ "$(cat "$OUT_DIR/failing-child.stderr")" == "fail-err" ]] || fail failing-child-stderr

    run_streaming_probe

    run_cmd egress-none m80 run --egress none -- /bin/true
    run_cmd egress-outbound m80 run --egress outbound -- /bin/true

    run_cmd list m80 list
    expect_exit inspect-missing 6 m80 inspect missing-smoke-vm
    run_cmd cleanup m80 cleanup

    expect_exit json-error 6 m80 --json run --scratch-size 0 -- /bin/true
    grep -q '"variant": "Config"' "$OUT_DIR/json-error.stderr" || fail json-error-variant

    expect_exit warm-no-owner 6 m80 run --warm -- /bin/true
    grep -q 'warm owner unavailable' "$OUT_DIR/warm-no-owner.stderr" || fail warm-no-owner-detail

    run_cmd pty-e2e cargo test -p m80-cli --test e2e_tty -- --ignored --nocapture
    run_cmd warm-e2e cargo test -p m80-cli --test e2e_warm -- --ignored --nocapture
}

run_streaming_probe() {
    echo "=== streaming probe ==="
    local stream_out="$OUT_DIR/streaming.stdout"
    local stream_err="$OUT_DIR/streaming.stderr"
    coproc STREAM {
        m80 run --egress none -- /bin/sh -c 'printf early; sleep 2; printf late' 2>"$stream_err"
    }
    local first
    if ! IFS= read -r -N 5 -t 5 first <&"${STREAM[0]}"; then
        kill "$STREAM_PID" 2>/dev/null || true
        fail streaming-first-chunk
    fi
    [[ "$first" == "early" ]] || {
        kill "$STREAM_PID" 2>/dev/null || true
        fail "streaming expected early got $first"
    }
    if ! kill -0 "$STREAM_PID" 2>/dev/null; then
        fail streaming-child-exited-before-late-output
    fi
    printf '%s' "$first" >"$stream_out"
    cat <&"${STREAM[0]}" >>"$stream_out" &
    local cat_pid=$!
    wait "$STREAM_PID" || fail streaming-wait
    wait "$cat_pid" || true
    [[ "$(cat "$stream_out")" == "earlylate" ]] || fail streaming-output
}

run_workspace_probe() {
    echo "=== workspace/writeback probe ==="
    local workspace="$OUT_DIR/workspace"
    mkdir -p "$workspace"
    printf workspace-in >"$workspace/input.txt"
    m80 run \
        --egress none \
        --workspace "$workspace" \
        --cwd /workspace \
        --scratch-size 67108864 \
        --writeback always \
        -- /bin/sh -c 'cat input.txt; printf workspace-err >&2; printf workspace-out > output.txt' \
        >"$OUT_DIR/workspace.stdout" 2>"$OUT_DIR/workspace.stderr" || fail workspace-probe
    [[ "$(cat "$OUT_DIR/workspace.stdout")" == "workspace-in" ]] || fail workspace-stdout
    [[ "$(cat "$OUT_DIR/workspace.stderr")" == "workspace-err" ]] || fail workspace-stderr
    [[ "$(cat "$workspace/output.txt")" == "workspace-out" ]] || fail workspace-writeback
}

build_cli
if [[ "$RUN_KVM" == "1" ]]; then
    run_kvm_smoke
else
    run_non_kvm_ci
    echo "KVM smoke skipped; set M80_RUN_KVM_E2E=1 to run ignored e2e probes."
fi
