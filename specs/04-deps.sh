#!/usr/bin/env bash
# Wire dependencies between m80 beads.
# Format: br dep add <issue> <depends-on>  (means "issue depends on depends-on")
# Read-only on the bead content; only adds edges. Idempotent? mostly — re-running is OK.
set -euo pipefail

cd /tank/projects/m80
. specs/skeleton-ids.env  # exposes $L1_xx, $L2_xx_y as shell vars

add() {
  echo "  $1 -> $2"
  br dep add "$1" "$2" 2>&1 || echo "    (already exists or failed)"
}

echo "Internal OutboundNat phase ordering"
add "$L2_11_2" "$L2_11_1"  # collision detection depends on address allocation
add "$L2_11_3" "$L2_11_2"  # bridge/tap setup depends on collision detection
add "$L2_11_4" "$L2_11_3"  # guest network injection depends on bridge/tap
add "$L2_11_5" "$L2_11_3"  # iptables policy depends on bridge/tap
add "$L2_11_6" "$L2_11_5"  # rule teardown depends on iptables policy
add "$L2_11_7" "$L2_11_3"  # bridge ownership recovery depends on bridge/tap

echo
echo "Cross-L1 epic ordering"
add "$L1_02" "$L1_01"  # boot lifecycle depends on preflight
add "$L1_09" "$L1_08"  # cgroup depends on jailer
add "$L1_04" "$L1_02"  # generic guest exec depends on boot lifecycle
add "$L1_06" "$L1_02"  # vsock channel depends on boot lifecycle
add "$L1_15" "$L1_02"  # cleanup/teardown depends on boot lifecycle
add "$L1_03" "$L1_02"  # storage flow depends on boot lifecycle
add "$L1_07" "$L1_01"  # image build depends on preflight (artifacts/manifest)

echo
echo "Done."
