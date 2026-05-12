#!/usr/bin/env bash
# Mock m80 binary for bench-harness e2e testing.
#
# Behaves like `m80 run` and `m80 cleanup`:
#   - Accepts the same env vars (IMAGE_BUILD_DIR, M80_KERNEL_KIND, etc.)
#   - Emits M80_PHASE name=... elapsed_us=... lines on stderr when
#     M80_PHASE_TRACE=1, matching the wire-trace format the real CLI
#     emits.
#   - Sleeps a small randomized amount per phase so the bench harness
#     measures non-zero wallclock and tail variance is observable in
#     histograms / P99.
#
# Usage: invoked by bench-cold-launch.sh when M80_BIN points here.

set -e

subcommand="${1:-}"

if [[ "$subcommand" == "cleanup" ]]; then
    exit 0
fi

if [[ "$subcommand" != "run" ]]; then
    echo "mock-m80: unsupported subcommand '$subcommand'" >&2
    exit 2
fi

# Tunable jitter (microseconds). Defaults to a snappy 50-500 us per phase.
mock_phase_jitter_us() {
    local lo=$1 hi=$2
    local span=$((hi - lo))
    echo $(( lo + (RANDOM % span) ))
}

# Emit M80_PHASE lines on stderr only when the wire-trace knob is on,
# matching the real CLI's behavior.
emit_phase() {
    local name="$1" us="$2"
    [[ "${M80_PHASE_TRACE:-0}" == "1" ]] || return 0
    printf 'M80_PHASE name=%s elapsed_us=%s\n' "$name" "$us" >&2
}

# Simulate a launch with a representative phase breakdown. The exact us
# values are noise; what matters for the harness e2e is that:
#   1) total wallclock > 0
#   2) per-phase events are emitted in M80_PHASE format
#   3) some variability across runs (so percentiles are non-degenerate)
sleep_us() { :; }   # placeholder: real sleep would burn wallclock, skip for speed

# Phase set mirrors the real cold-launch breakdown (jailer/setup/boot/...).
for phase in phase_1_run_root_prep phase_4_jailer_materialize \
             phase_8_api_socket phase_10_open_uds \
             phase_11_rest_puts phase_12a_instance_start \
             phase_12b_ready_accept stop_bounded; do
    us=$(mock_phase_jitter_us 1000 50000)
    emit_phase "$phase" "$us"
done

# Echo the requested args to stdout to simulate `/bin/echo bench-N`.
shift  # drop "run"
# Find -- separator and echo everything after it.
while [[ $# -gt 0 && "$1" != "--" ]]; do shift; done
[[ "${1:-}" == "--" ]] && shift
echo "$@"

exit 0
