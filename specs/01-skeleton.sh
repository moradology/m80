#!/usr/bin/env bash
# Skeleton authoring script: creates 18 L1 epics + 72 L2 sub-epics for m80.
# Run from /tank/projects/m80/. Idempotent? NO — re-running creates duplicates.
# Output: ids written to /tank/projects/m80/specs/skeleton-ids.env so leaf
# scripts can re-use them.
set -euo pipefail

cd /tank/projects/m80

ID_FILE="specs/skeleton-ids.env"
: > "$ID_FILE"
emit() { echo "$1=$2" >> "$ID_FILE"; }

# Standard label sets
ACTIVE="firecracker,behavior-capture,active"
DEFERRED_V02="firecracker,behavior-capture,deferred-v02"
DEFERRED_DEAD="firecracker,behavior-capture,deferred-deadcode"

############################################################
# L1-01 Host Preflight & Discovery
############################################################
L1_01=$(br create "Host Preflight & Discovery" \
  --type epic --priority 1 \
  --labels "$ACTIVE,preflight" \
  --description "Purpose: discover Firecracker/jailer binaries and validate the host can boot a microVM at all.
Contract: fail-closed on missing binary, incompatible Firecracker version, unwritable /dev/kvm, missing kernel modules, unwritable run-root, or missing storage helpers. No silent fallback.
Surface: foundation.rs (~2475 LOC), firecracker-preflight.sh, run_privileged_host_command shim.
Evidence: m80/docs/behaviors/preflight/ + m80/<crate>/tests/preflight/." \
  --silent)
emit L1_01 "$L1_01"

L2_01_1=$(br create "Binary discovery" --type epic --priority 1 --parent "$L1_01" \
  --labels "$ACTIVE,preflight,binaries" \
  --description "Contract: locate firecracker and jailer via env (FIRECRACKER_BIN/JAILER_BIN) or /opt/firecracker/bin defaults; probe --version against pinned FIRECRACKER_VERSION; missing binary or version mismatch is a hard error.
Surface: foundation.rs DiscoveryConfig, version probe.
Evidence: m80/docs/behaviors/preflight/binary-discovery.md + m80/<crate>/tests/preflight/binary_discovery.rs." \
  --silent)
emit L2_01_1 "$L2_01_1"

L2_01_2=$(br create "KVM and OS gates" --type epic --priority 1 --parent "$L1_01" \
  --labels "$ACTIVE,preflight,kvm" \
  --description "Contract: only Linux hosts; /dev/kvm exists and is writable (or sudo-writable for jailer); bridge+tap kernel modules present; effective root or passwordless sudo; checked once at startup.
Surface: foundation.rs verify_jailer_launch_privilege:847.
Evidence: m80/docs/behaviors/preflight/kvm-gates.md + m80/<crate>/tests/preflight/kvm_gates.rs." \
  --silent)
emit L2_01_2 "$L2_01_2"

L2_01_3=$(br create "Artifact and manifest preflight" --type epic --priority 1 --parent "$L1_01" \
  --labels "$ACTIVE,preflight,manifest" \
  --description "Contract: kernel auto-discovered via vmlinux-* sort; rootfs path absolute; .manifest.json passes schema validation; sha256 fields recomputed and matched; run-root absolute and creatable; mkfs.ext4/debugfs/e2fsck on PATH; tabular pass/fail report; any failure is fatal.
Surface: foundation.rs, prepare-guestd-image.sh manifest output.
Evidence: m80/docs/behaviors/preflight/manifest.md + m80/<crate>/tests/preflight/manifest.rs." \
  --silent)
emit L2_01_3 "$L2_01_3"

L2_01_4=$(br create "Privileged-command shim" --type epic --priority 1 --parent "$L1_01" \
  --labels "$ACTIVE,preflight,privilege" \
  --description "Contract: run_privileged_host_command() switches between direct exec (euid=0) and sudo -n; covers ip/iptables/sysctl only; password prompts cause hard failure; centralized — no module shells sudo directly.
Surface: foundation.rs:860-879.
Evidence: m80/docs/behaviors/preflight/privileged-shim.md + m80/<crate>/tests/preflight/privileged_shim.rs." \
  --silent)
emit L2_01_4 "$L2_01_4"

############################################################
# L1-02 VM Boot Lifecycle
############################################################
L1_02=$(br create "VM Boot Lifecycle" --type epic --priority 1 \
  --labels "$ACTIVE,lifecycle" \
  --description "Purpose: drive a single microVM through create/start/stop/delete with run-root persistence and ownership.
Contract: strict ordered preboot pipeline; ready detection bounded; graceful stop arch-sensitive; delete idempotent and orphan-recoverable.
Surface: lifecycle.rs (~1717 LOC), boot.rs (~1226), client.rs (~896).
Evidence: m80/docs/behaviors/lifecycle/ + m80/<crate>/tests/lifecycle/." \
  --silent)
emit L1_02 "$L1_02"

L2_02_1=$(br create "Run-directory layout" --type epic --priority 1 --parent "$L1_02" \
  --labels "$ACTIVE,lifecycle,run-dir" \
  --description "Contract: per-VM directory keyed by VM ID under run-root; firecracker.sock + vsock.sock + state files co-located; reaped on clean delete; orphans scavenged on startup.
Surface: lifecycle.rs run-dir creation, recover_stale_run_root.
Evidence: m80/docs/behaviors/lifecycle/run-dir.md + m80/<crate>/tests/lifecycle/run_dir.rs." \
  --silent)
emit L2_02_1 "$L2_02_1"

L2_02_2=$(br create "Pre-boot wiring (boot.rs)" --type epic --priority 1 --parent "$L1_02" \
  --labels "$ACTIVE,lifecycle,boot" \
  --description "Contract: machine config + boot source + root drive + optional scratch drive + vsock device PUT to API before InstanceStart; NIC iff OutboundNat resolved; boot-identity.json recorded on success.
Surface: boot.rs, client.rs:BootSourceConfig.
Evidence: m80/docs/behaviors/lifecycle/preboot.md + m80/<crate>/tests/lifecycle/preboot.rs." \
  --silent)
emit L2_02_2 "$L2_02_2"

L2_02_3=$(br create "UDS REST API client" --type epic --priority 1 --parent "$L1_02" \
  --labels "$ACTIVE,lifecycle,api-client" \
  --description "Contract: synchronous HTTP-over-UDS; speaks BootSource/Drive/NetworkInterface/MachineConfig/Vsock/InstanceAction; single-threaded blocking; concurrency guarded by lifecycle phasing; typed error variants on API failures.
Surface: client.rs.
Evidence: m80/docs/behaviors/lifecycle/uds-client.md + m80/<crate>/tests/lifecycle/uds_client.rs." \
  --silent)
emit L2_02_3 "$L2_02_3"

L2_02_4=$(br create "Start sequence and ready detection" --type epic --priority 1 --parent "$L1_02" \
  --labels "$ACTIVE,lifecycle,start" \
  --description "Contract: InstanceAction::InstanceStart triggers boot; host polls serial console for GUESTD_READY marker; bounded timeout maps to VsockNotReady; one-shot startup; full boot/stop per execute call (no warm pool in v0).
Surface: lifecycle.rs, vsock.rs.
Evidence: m80/docs/behaviors/lifecycle/start.md + m80/<crate>/tests/lifecycle/start.rs." \
  --silent)
emit L2_02_4 "$L2_02_4"

L2_02_5=$(br create "Graceful stop" --type epic --priority 1 --parent "$L1_02" \
  --labels "$ACTIVE,lifecycle,stop" \
  --description "Contract: x86_64 sends SendCtrlAltDel and waits ~30s; aarch64 falls through to forced kill; timeout escalates to SIGKILL of firecracker AND jailer pids; idempotent re-invoke; console log flushed before delete.
Surface: lifecycle.rs:1297-1308, jailer.rs.
Evidence: m80/docs/behaviors/lifecycle/stop.md + m80/<crate>/tests/lifecycle/stop.rs." \
  --silent)
emit L2_02_5 "$L2_02_5"

L2_02_6=$(br create "Delete and run-root recovery" --type epic --priority 1 --parent "$L1_02" \
  --labels "$ACTIVE,lifecycle,delete,recovery" \
  --description "Contract: delete tears down sockets, taps (if any), state files, run-dir; background recover_stale_run_root scans every 5s and reaps orphans; lease/ownership checks before deletion; ambiguous state preserves residue; bridge orphans only removed when no peer references them.
Surface: lifecycle.rs:1477, network.rs cleanup_orphan_bridge_if_unused.
Evidence: m80/docs/behaviors/lifecycle/delete.md + m80/<crate>/tests/lifecycle/delete.rs." \
  --silent)
emit L2_02_6 "$L2_02_6"

############################################################
# L1-03 Storage & Filesystem
############################################################
L1_03=$(br create "Storage & Filesystem" --type epic --priority 1 \
  --labels "$ACTIVE,storage" \
  --description "Purpose: per-VM rootfs cloning, scratch image hydration, post-stop change extraction (opt-in, generic — not gated on EffectClass).
Contract: clones isolated per VM; hydration opaque to guest; extraction post-stop only; admissibility scan rejects symlinks/specials; atomic swap with rollback on failure.
Surface: storage.rs (~992 LOC).
Evidence: m80/docs/behaviors/storage/ + m80/<crate>/tests/storage/." \
  --silent)
emit L1_03 "$L1_03"

L2_03_1=$(br create "Per-VM rootfs cloning" --type epic --priority 1 --parent "$L1_03" \
  --labels "$ACTIVE,storage,rootfs" \
  --description "Contract: base ext4 cloned per VM under run-dir; base sha256 verified against manifest before cloning; OutboundNat injects systemd-network units into clone before boot; immutable base preserved.
Surface: storage.rs.
Evidence: m80/docs/behaviors/storage/rootfs-cloning.md + m80/<crate>/tests/storage/rootfs_cloning.rs." \
  --silent)
emit L2_03_1 "$L2_03_1"

L2_03_2=$(br create "Scratch workspace image" --type epic --priority 1 --parent "$L1_03" \
  --labels "$ACTIVE,storage,scratch" \
  --description "Contract: opt-in per-VM scratch ext4 created via mkfs.ext4 when workspace supplied; host workspace tree hydrated before boot; opaque to guest; mounted at fixed in-guest path via systemd .mount unit; appears as /dev/vdb; sized for expected mutations.
Surface: storage.rs.
Evidence: m80/docs/behaviors/storage/scratch.md + m80/<crate>/tests/storage/scratch.rs." \
  --silent)
emit L2_03_2 "$L2_03_2"

L2_03_3=$(br create "Change extraction (opt-in)" --type epic --priority 1 --parent "$L1_03" \
  --labels "$ACTIVE,storage,extraction" \
  --description "Contract: post-clean-stop e2fsck repairs scratch journal; debugfs extracts modified file set without remounting; staging tree before atomic swap; extraction failure triggers full rollback; gated on caller request only — NOT on EffectClass (agent concern).
Surface: storage.rs.
Evidence: m80/docs/behaviors/storage/extraction.md + m80/<crate>/tests/storage/extraction.rs." \
  --silent)
emit L2_03_3 "$L2_03_3"

L2_03_4=$(br create "Admissibility and atomic swap" --type epic --priority 1 --parent "$L1_03" \
  --labels "$ACTIVE,storage,admissibility" \
  --description "Contract: only regular files and directories survive; symlinks rejected; special files (devices, fifos, sockets) rejected; staged tree atomically renamed into host workspace; any swap failure triggers full rollback.
Surface: storage.rs admissibility scan.
Evidence: m80/docs/behaviors/storage/admissibility.md + m80/<crate>/tests/storage/admissibility.rs." \
  --silent)
emit L2_03_4 "$L2_03_4"

############################################################
# L1-04 Generic Guest Exec Service
############################################################
L1_04=$(br create "Generic Guest Exec Service" --type epic --priority 1 \
  --labels "$ACTIVE,guest-exec" \
  --description "Purpose: in-VM daemon listening on vsock that runs argv with optional cwd/env and returns stdout/stderr/exit/timing. Generic — no tool catalog, no policy enforcement (agent concerns).
Contract: one connection per request; envelope read → exec → response → fs sync → close; bounded timeout produces TimedOut; exit code collected; cancellation via connection close kills child.
Surface: services/guestd-rs/main.rs (reframed for generic), agent-guestd-lib/dispatch.rs.
Evidence: m80/docs/behaviors/guest-exec/ + m80/<crate>/tests/guest-exec/." \
  --silent)
emit L1_04 "$L1_04"

L2_04_1=$(br create "Image contents and systemd boot" --type epic --priority 1 --parent "$L1_04" \
  --labels "$ACTIVE,guest-exec,image" \
  --description "Contract: m80 daemon binary at /usr/local/bin/<name>; systemd unit Type=simple Restart=on-failure; mount unit for scratch device; environment file; multi-user.target boot; both units enabled in multi-user.target.wants.
Surface: prepare-guestd-image.sh installation steps.
Evidence: m80/docs/behaviors/guest-exec/systemd-boot.md + m80/<crate>/tests/guest-exec/systemd_boot.rs." \
  --silent)
emit L2_04_1 "$L2_04_1"

L2_04_2=$(br create "Vsock listener and accept loop" --type epic --priority 1 --parent "$L1_04" \
  --labels "$ACTIVE,guest-exec,vsock" \
  --description "Contract: bind vsock port 9001 in guest; one TCP-style accept per request; per-connection sequence: read envelope → dispatch exec → serialize response → flush filesystems → close.
Surface: services/guestd-rs/main.rs accept loop.
Evidence: m80/docs/behaviors/guest-exec/accept-loop.md + m80/<crate>/tests/guest-exec/accept_loop.rs." \
  --silent)
emit L2_04_2 "$L2_04_2"

L2_04_3=$(br create "Process orchestration" --type epic --priority 1 --parent "$L1_04" \
  --labels "$ACTIVE,guest-exec,process" \
  --description "Contract: child spawned with stdout/stderr captured; bounded timeout (timeout_ms) enforced; expiry yields ExecutionStatus::TimedOut; SIGKILL on timeout; exit code collected and returned in response; partial output flushed even on failure.
Surface: agent-guestd-lib process orchestration.
Evidence: m80/docs/behaviors/guest-exec/process.md + m80/<crate>/tests/guest-exec/process.rs." \
  --silent)
emit L2_04_3 "$L2_04_3"

L2_04_4=$(br create "Cancellation" --type epic --priority 1 --parent "$L1_04" \
  --labels "$ACTIVE,guest-exec,cancellation" \
  --description "Contract: connection close from host kills running child; partial stdout/stderr flushed; ExecutionStatus::Cancelled returned if response can still be sent before close completes.
Surface: services/guestd-rs/main.rs connection lifecycle.
Evidence: m80/docs/behaviors/guest-exec/cancellation.md + m80/<crate>/tests/guest-exec/cancellation.rs." \
  --silent)
emit L2_04_4 "$L2_04_4"

############################################################
# L1-05 Generic Wire Protocol
############################################################
L1_05=$(br create "Generic Wire Protocol" --type epic --priority 1 \
  --labels "$ACTIVE,wire-protocol" \
  --description "Purpose: opaque request/response wire over vsock with NDJSON framing and version handshake. Generic — no tool_call_id, correlation_id, or idempotency_key semantics (agent concerns); at most an opaque request_id round-trip.
Contract: NDJSON one-doc-per-line with 4 MiB ceiling; version byte explicit; mismatch fails closed.
Surface: agent-guest-proto/envelope.rs (reframed).
Evidence: m80/docs/behaviors/wire-protocol/ + m80/<crate>/tests/wire-protocol/." \
  --silent)
emit L1_05 "$L1_05"

L2_05_1=$(br create "Envelope schema" --type epic --priority 1 --parent "$L1_05" \
  --labels "$ACTIVE,wire-protocol,envelope" \
  --description "Contract: explicit version byte = 1; opaque request payload + response payload; optional request_id echoed unchanged in response.
Surface: agent-guest-proto envelope types (generic-only fields).
Evidence: m80/docs/behaviors/wire-protocol/envelope.md + m80/<crate>/tests/wire-protocol/envelope.rs." \
  --silent)
emit L2_05_1 "$L2_05_1"

L2_05_2=$(br create "Frame transport" --type epic --priority 1 --parent "$L1_05" \
  --labels "$ACTIVE,wire-protocol,framing" \
  --description "Contract: one JSON document per line; per-frame size ceiling 4 MiB; reads line-buffered with partial frames accumulated and parsed; parse failure terminates connection.
Surface: agent-guest-proto/envelope.rs:22 size constant; main.rs frame loop.
Evidence: m80/docs/behaviors/wire-protocol/framing.md + m80/<crate>/tests/wire-protocol/framing.rs." \
  --silent)
emit L2_05_2 "$L2_05_2"

L2_05_3=$(br create "Version handshake" --type epic --priority 1 --parent "$L1_05" \
  --labels "$ACTIVE,wire-protocol,handshake" \
  --description "Contract: on connect host and guest exchange protocol version; mismatched versions fail closed with IncompatibleVersion error variant; handshake precedes any exec dispatch.
Surface: services/guestd-rs/main.rs:428-439.
Evidence: m80/docs/behaviors/wire-protocol/handshake.md + m80/<crate>/tests/wire-protocol/handshake.rs." \
  --silent)
emit L2_05_3 "$L2_05_3"

############################################################
# L1-06 Vsock Channel
############################################################
L1_06=$(br create "Vsock Channel" --type epic --priority 1 \
  --labels "$ACTIVE,vsock" \
  --description "Purpose: host-side bridge between Firecracker UDS and guest vsock; ready-marker probe via serial console.
Contract: one connection per request; CID derived from VM id; guest port fixed at 9001; UDS removed on delete.
Surface: vsock.rs (~410 LOC), lib.rs:113-116 (DEFAULT_GUESTD_*).
Evidence: m80/docs/behaviors/vsock/ + m80/<crate>/tests/vsock/." \
  --silent)
emit L1_06 "$L1_06"

L2_06_1=$(br create "CID and port allocation" --type epic --priority 1 --parent "$L1_06" \
  --labels "$ACTIVE,vsock,allocation" \
  --description "Contract: each VM assigned a vsock CID derived from vm_id; guest port fixed at 9001; host-side bridge UDS at <run_dir>/vsock.sock.
Surface: vsock.rs allocation; lib.rs DEFAULT_GUESTD_GUEST_CID/_PORT.
Evidence: m80/docs/behaviors/vsock/cid-port.md + m80/<crate>/tests/vsock/cid_port.rs." \
  --silent)
emit L2_06_1 "$L2_06_1"

L2_06_2=$(br create "Ready-marker probe" --type epic --priority 1 --parent "$L1_06" \
  --labels "$ACTIVE,vsock,ready" \
  --description "Contract: host watches serial console for GUESTD_READY marker; marker string configurable in manifest; timeout maps to VsockNotReady; probe runs after InstanceStart and before any exec dispatch.
Surface: vsock.rs probe; lifecycle.rs:697 wait_for_console_marker.
Evidence: m80/docs/behaviors/vsock/ready-probe.md + m80/<crate>/tests/vsock/ready_probe.rs." \
  --silent)
emit L2_06_2 "$L2_06_2"

L2_06_3=$(br create "Connection lifecycle" --type epic --priority 1 --parent "$L1_06" \
  --labels "$ACTIVE,vsock,connection" \
  --description "Contract: one connection opened per request, used once, closed; concurrent connections to same VM unsupported in v0; vsock socket removed during VM delete.
Surface: vsock.rs connection management.
Evidence: m80/docs/behaviors/vsock/connection.md + m80/<crate>/tests/vsock/connection.rs." \
  --silent)
emit L2_06_3 "$L2_06_3"

############################################################
# L1-07 Image Build Pipeline
############################################################
L1_07=$(br create "Image Build Pipeline" --type epic --priority 1 \
  --labels "$ACTIVE,image-build" \
  --description "Purpose: build the guest image (kernel + rootfs + provenance manifest). Generic — drops interpreter opinions (no Python/Node/npm policy).
Contract: kernel from firecracker-ci S3; rootfs squashfs→ext4 resized; chroot installs m80 daemon + systemd units; provenance manifest is sha256-keyed and verified at boot.
Surface: prepare-guestd-image.sh (~326 lines reframed).
Evidence: m80/docs/behaviors/image-build/ + m80/<crate>/tests/image-build/." \
  --silent)
emit L1_07 "$L1_07"

L2_07_1=$(br create "Source artifact acquisition" --type epic --priority 1 --parent "$L1_07" \
  --labels "$ACTIVE,image-build,artifacts" \
  --description "Contract: kernel from s3.amazonaws.com/spec.ccfc.min/firecracker-ci/<version>/<arch> sorted by version; rootfs from same bucket as squashfs; squashfs converted/expanded to ext4 resized to 1 GiB via truncate; firecracker+jailer from GitHub releases firecracker-microvm/firecracker.
Surface: prepare-guestd-image.sh acquisition stage.
Evidence: m80/docs/behaviors/image-build/acquisition.md + m80/<crate>/tests/image-build/acquisition.rs." \
  --silent)
emit L2_07_1 "$L2_07_1"

L2_07_2=$(br create "Chroot customization" --type epic --priority 1 --parent "$L1_07" \
  --labels "$ACTIVE,image-build,chroot" \
  --description "Contract: ext4 mounted via loop device; chroot installs m80 daemon binary at /usr/local/bin/<name>; systemd units installed and enabled; workspace mount-point directory created. NO opinionated interpreter set (no Python/Node, no npm/pip detection policy).
Surface: prepare-guestd-image.sh chroot stage (reframed).
Evidence: m80/docs/behaviors/image-build/chroot.md + m80/<crate>/tests/image-build/chroot.rs." \
  --silent)
emit L2_07_2 "$L2_07_2"

L2_07_3=$(br create "Provenance manifest" --type epic --priority 1 --parent "$L1_07" \
  --labels "$ACTIVE,image-build,manifest" \
  --description "Contract: <rootfs>.manifest.json next to ext4; schema_version: 1; sha256 over kernel/source-rootfs/output-rootfs/daemon-binary/units; expected_firecracker_version recorded; ready_marker/guest_port/boot_target/no_egress_reason recorded as load-bearing config.
Surface: prepare-guestd-image.sh manifest emission.
Evidence: m80/docs/behaviors/image-build/manifest.md + m80/<crate>/tests/image-build/manifest.rs." \
  --silent)
emit L2_07_3 "$L2_07_3"

L2_07_4=$(br create "Manifest verification at boot" --type epic --priority 1 --parent "$L1_07" \
  --labels "$ACTIVE,image-build,verification" \
  --description "Contract: preflight runs schema validator over manifest; all sha256 fields recomputed and compared; tampering detected; stale hashes refuse boot.
Surface: foundation.rs manifest validation, prepare-guestd-image.sh schema.
Evidence: m80/docs/behaviors/image-build/verification.md + m80/<crate>/tests/image-build/verification.rs." \
  --silent)
emit L2_07_4 "$L2_07_4"

############################################################
# L1-08 Jailer & Privilege Drop
############################################################
L1_08=$(br create "Jailer & Privilege Drop" --type epic --priority 1 \
  --labels "$ACTIVE,jailer" \
  --description "Purpose: materialize Firecracker's official jailer chroot per VM with privilege drop and replayable plan.
Contract: per-VM chroot under jail root; assets bound RO/RW or created inside jail; UID/GID configurable; jailer+firecracker pids both tracked; startup verifies launch privilege; stale jailers scavenged on startup.
Surface: jailer.rs (~1457 LOC).
Evidence: m80/docs/behaviors/jailer/ + m80/<crate>/tests/jailer/." \
  --silent)
emit L1_08 "$L1_08"

L2_08_1=$(br create "Jail root layout" --type epic --priority 1 --parent "$L1_08" \
  --labels "$ACTIVE,jailer,layout" \
  --description "Contract: jail root under /var/tmp/m80-fc-jailer (or run-root sibling); chroot materialized per VM; jailer-plan.json persisted for inspection; jailer-state.json holds runtime state.
Surface: jailer.rs layout types.
Evidence: m80/docs/behaviors/jailer/layout.md + m80/<crate>/tests/jailer/layout.rs." \
  --silent)
emit L2_08_1 "$L2_08_1"

L2_08_2=$(br create "Asset binding plan" --type epic --priority 1 --parent "$L1_08" \
  --labels "$ACTIVE,jailer,binding" \
  --description "Contract: kernel and read-only host artifacts use BindRo; per-VM run-dir uses BindRw; sockets (API, vsock) use CreateInsideJail; plan computed pre-launch; binding happens during materialize; plan replayable for triage.
Surface: jailer.rs binding plan.
Evidence: m80/docs/behaviors/jailer/binding.md + m80/<crate>/tests/jailer/binding.rs." \
  --silent)
emit L2_08_2 "$L2_08_2"

L2_08_3=$(br create "Privilege drop" --type epic --priority 1 --parent "$L1_08" \
  --labels "$ACTIVE,jailer,privilege" \
  --description "Contract: jailed UID/GID configurable (was fixed 3000/3000 in predecessor); jailer_pid AND firecracker_pid tracked separately for kill targeting and recovery; verify_jailer_launch_privilege runs once at startup; PrivilegedJailerLaunchUnavailable on insufficient privilege.
Surface: jailer.rs, foundation.rs:847.
Evidence: m80/docs/behaviors/jailer/privilege.md + m80/<crate>/tests/jailer/privilege.rs." \
  --silent)
emit L2_08_3 "$L2_08_3"

L2_08_4=$(br create "Scavenge on startup" --type epic --priority 1 --parent "$L1_08" \
  --labels "$ACTIVE,jailer,scavenge" \
  --description "Contract: tracked jailer/firecracker pids identify orphan jailers from prior crashes; integrated with run-root recovery loop (5s); non-destructive on ambiguity (preserves uncertain residue).
Surface: lifecycle.rs recover_stale_run_root, jailer.rs recover_jailer_from_run_dir.
Evidence: m80/docs/behaviors/jailer/scavenge.md + m80/<crate>/tests/jailer/scavenge.rs." \
  --silent)
emit L2_08_4 "$L2_08_4"

############################################################
# L1-09 Cgroup v2 Limits
############################################################
L1_09=$(br create "Cgroup v2 Limits" --type epic --priority 1 \
  --labels "$ACTIVE,cgroup" \
  --description "Purpose: per-VM cgroup v2 subtree for CPU/memory/pids enforcement, hygiene-grade.
Contract: subtree under /sys/fs/cgroup/m80-firecracker; both jailer and firecracker pids assigned; mode gated by M80_CGROUP_MODE=unified-v2; non-unified hosts skip; cleanup integrated with VM delete.
Surface: cgroup.rs (~278 LOC).
Evidence: m80/docs/behaviors/cgroup/ + m80/<crate>/tests/cgroup/." \
  --silent)
emit L1_09 "$L1_09"

L2_09_1=$(br create "Subtree creation" --type epic --priority 1 --parent "$L1_09" \
  --labels "$ACTIVE,cgroup,subtree" \
  --description "Contract: subtree under /sys/fs/cgroup/m80-firecracker; both jailer and firecracker pids assigned to subtree; materialized cgroup path persisted for cleanup.
Surface: cgroup.rs subtree creation.
Evidence: m80/docs/behaviors/cgroup/subtree.md + m80/<crate>/tests/cgroup/subtree.rs." \
  --silent)
emit L2_09_1 "$L2_09_1"

L2_09_2=$(br create "Limit enforcement" --type epic --priority 1 --parent "$L1_09" \
  --labels "$ACTIVE,cgroup,limits" \
  --description "Contract: CPU, memory, and pids limited via cgroup v2 controllers; mode gated by M80_CGROUP_MODE=unified-v2 env; hosts not in unified-v2 mode skip cgroup limits entirely.
Surface: cgroup.rs limit enforcement.
Evidence: m80/docs/behaviors/cgroup/limits.md + m80/<crate>/tests/cgroup/limits.rs." \
  --silent)
emit L2_09_2 "$L2_09_2"

L2_09_3=$(br create "Failure modes" --type epic --priority 1 --parent "$L1_09" \
  --labels "$ACTIVE,cgroup,errors" \
  --description "Contract: CgroupRequiresJailer is the typed error when cgroup mode is requested without jailer; cgroup cleanup is part of VM delete; persisted cgroup path used for cleanup.
Surface: errors.rs CgroupRequiresJailer; cgroup.rs cleanup.
Evidence: m80/docs/behaviors/cgroup/failure-modes.md + m80/<crate>/tests/cgroup/failure_modes.rs." \
  --silent)
emit L2_09_3 "$L2_09_3"

############################################################
# L1-10 Networking — NoEgress
############################################################
L1_10=$(br create "Networking — NoEgress" --type epic --priority 1 \
  --labels "$ACTIVE,network,no-egress" \
  --description "Purpose: default network mode — no NIC beyond loopback; no bridge, tap, or NAT rules.
Contract: boot config does not configure a NIC; no iptables modifications; no_egress_reason recorded in manifest; capability resolution collapses to a bool for m80 (no agent CapabilityClass plumbing).
Surface: network.rs:39-43,155-169.
Evidence: m80/docs/behaviors/network-no-egress/ + m80/<crate>/tests/network-no-egress/." \
  --silent)
emit L1_10 "$L1_10"

L2_10_1=$(br create "NoEgress configuration" --type epic --priority 1 --parent "$L1_10" \
  --labels "$ACTIVE,network,no-egress" \
  --description "Contract: no bridge, tap, or NAT rule created; boot config does not configure a NIC; guest sees only loopback; no iptables modification; no_egress_reason string recorded in manifest for downstream tooling.
Surface: network.rs NoEgress branch.
Evidence: m80/docs/behaviors/network-no-egress/configuration.md + m80/<crate>/tests/network-no-egress/configuration.rs." \
  --silent)
emit L2_10_1 "$L2_10_1"

L2_10_2=$(br create "Mode resolution" --type epic --priority 1 --parent "$L1_10" \
  --labels "$ACTIVE,network,resolution" \
  --description "Contract: resolve_vm_network_mode() is the single seam producing VmNetworkMode; for m80 the resolver collapses to a bool (NoEgress vs OutboundNat); lifecycle code consumes only the resolved mode.
Surface: network.rs:155-169 resolve_vm_network_mode.
Evidence: m80/docs/behaviors/network-no-egress/resolution.md + m80/<crate>/tests/network-no-egress/resolution.rs." \
  --silent)
emit L2_10_2 "$L2_10_2"

############################################################
# L1-11 Networking — OutboundNat (largest L1)
############################################################
L1_11=$(br create "Networking — OutboundNat" --type epic --priority 1 \
  --labels "$ACTIVE,network,outbound-nat" \
  --description "Purpose: deterministic per-host bridge/tap/IP/MAC allocation, default-deny iptables policy with admitted DNS resolvers and bounded private exceptions, ownership-aware idempotent cleanup.
Contract: bridge/tap names + CIDR + IPv4 + MAC are deterministic functions of (run_root, vm_id) via sha256; collisions fail closed; rules tagged for ownership cleanup; bridge orphans only removed when no peer references them.
Surface: network.rs (3669 LOC, dossier risk #1). Spec'd in v0.1; implementation deferred to v0.2.
Evidence: m80/docs/behaviors/network-outbound-nat/ + m80/<crate>/tests/network-outbound-nat/." \
  --silent)
emit L1_11 "$L1_11"

L2_11_1=$(br create "Address allocation" --type epic --priority 1 --parent "$L1_11" \
  --labels "$ACTIVE,network,outbound-nat,allocation" \
  --description "Contract: bridge name brfc + first 12 hex of sha256(run_root); bridge CIDR 172.<o2>.<o3>.0/24 from sha256 bytes (o2 = (sha256[0]%16)+16, o3 = sha256[1]); cap of ~4096 non-colliding bridges per host (172.16.0.0/12); tap name tfc + first 12 hex of sha256(run_root+vm_id); guest IPv4 deterministic per (run_root,vm_id); guest MAC 02:+hex of vm_digest[0..5] locally-administered.
Surface: network.rs:Phase 1 functions.
Evidence: m80/docs/behaviors/network-outbound-nat/allocation.md + m80/<crate>/tests/network-outbound-nat/allocation.rs." \
  --silent)
emit L2_11_1 "$L2_11_1"

L2_11_2=$(br create "Collision detection" --type epic --priority 1 --parent "$L1_11" \
  --labels "$ACTIVE,network,outbound-nat,collision" \
  --description "Contract: reject_guest_ipv4_collision scans <run_root>/*/network-state.json for any other VM with this guest IP; reject_host_route_collision reads /proc/net/route and rejects bridge CIDRs overlapping any existing route; collision is a hard error with no random fallback.
Surface: network.rs Phase 2 collision functions.
Evidence: m80/docs/behaviors/network-outbound-nat/collision.md + m80/<crate>/tests/network-outbound-nat/collision.rs." \
  --silent)
emit L2_11_2 "$L2_11_2"

L2_11_3=$(br create "Bridge and tap setup" --type epic --priority 1 --parent "$L1_11" \
  --labels "$ACTIVE,network,outbound-nat,setup" \
  --description "Contract: bridge setup idempotent (skip if exists with matching ownership); bridge owner recorded in run-root-level outbound-bridge-state.json; tap created with ip tuntap add ... mode tap and attached to bridge; per-VM network-state.json written atomically; all ip invocations via privileged-command shim.
Surface: network.rs Phase 3 bridge/tap setup.
Evidence: m80/docs/behaviors/network-outbound-nat/setup.md + m80/<crate>/tests/network-outbound-nat/setup.rs." \
  --silent)
emit L2_11_3 "$L2_11_3"

L2_11_4=$(br create "Guest network injection" --type epic --priority 1 --parent "$L1_11" \
  --labels "$ACTIVE,network,outbound-nat,injection" \
  --description "Contract: DNS resolvers discovered via resolvectl dns first then /etc/resolv.conf; filtered by is_admitted_dns_resolver to public-IPv4 only (rejects private/link-local/loopback/CGN/doc-ranges/multicast); systemd-network unit /etc/systemd/network/10-m80-outbound.network written into per-VM rootfs clone with static IPv4/gateway/MAC; resolved drop-in /etc/systemd/resolved.conf.d/10-m80-dns.conf written; guest daemon does NOT touch networking — systemd-networkd does the work at boot.
Surface: network.rs Phase 5 injection.
Evidence: m80/docs/behaviors/network-outbound-nat/injection.md + m80/<crate>/tests/network-outbound-nat/injection.rs." \
  --silent)
emit L2_11_4 "$L2_11_4"

L2_11_5=$(br create "iptables policy" --type epic --priority 1 --parent "$L1_11" \
  --labels "$ACTIVE,network,outbound-nat,iptables" \
  --description "Contract: sysctl net.ipv4.ip_forward=1 set before applying rules; per-VM filter chain tfw<12-hex>; admitted-DNS accept rules (UDP+TCP port 53); all other port-53 traffic REJECT; bounded private exception CIDRs accepted; permanent-deny list (bridge CIDR, link-local 169.254/16, loopback 127/8); default ACCEPT after deny-list; FORWARD entry routes from bridge with guest IP into per-VM chain; NAT POSTROUTING masquerade for guest IP.
Surface: network.rs Phase 6 iptables.
Evidence: m80/docs/behaviors/network-outbound-nat/iptables.md + m80/<crate>/tests/network-outbound-nat/iptables.rs." \
  --silent)
emit L2_11_5 "$L2_11_5"

L2_11_6=$(br create "Rule tagging and teardown" --type epic --priority 1 --parent "$L1_11" \
  --labels "$ACTIVE,network,outbound-nat,teardown" \
  --description "Contract: all rules carry per-VM comment prefix M80_RULE_COMMENT_PREFIX so cleanup finds them by comment match; cleanup_outbound_nat_policy deletes forwarding entries by comment; NAT masquerade deleted; filter chain rules deleted then chain itself when empty; tap deleted via delete_*_if_present wrappers tolerant of repeated calls.
Surface: network.rs:30 prefix constant; cleanup_outbound_nat_policy.
Evidence: m80/docs/behaviors/network-outbound-nat/teardown.md + m80/<crate>/tests/network-outbound-nat/teardown.rs." \
  --silent)
emit L2_11_6 "$L2_11_6"

L2_11_7=$(br create "Bridge ownership and orphan recovery" --type epic --priority 1 --parent "$L1_11" \
  --labels "$ACTIVE,network,outbound-nat,recovery" \
  --description "Contract: bridge removed only when no peer VM in run-root references it; cleanup_orphan_bridge_if_unused scavenges orphans at startup; recovery tolerant of missing/malformed state files; ambiguous residue preserved; crash-mid-VM does not break new VMs (next startup scavenges).
Surface: network.rs cleanup_orphan_bridge_if_unused.
Evidence: m80/docs/behaviors/network-outbound-nat/recovery.md + m80/<crate>/tests/network-outbound-nat/recovery.rs." \
  --silent)
emit L2_11_7 "$L2_11_7"

############################################################
# L1-12 Configuration & Discovery
############################################################
L1_12=$(br create "Configuration & Discovery" --type epic --priority 1 \
  --labels "$ACTIVE,configuration" \
  --description "Purpose: env var schema, config file precedence, backend discovery construction. Generic — no agent ID threading.
Contract: defaults → /etc/m80/config.toml → ~/.config/m80/config.toml → env → CLI flags; m80 config show reveals effective config; backend built once at startup and reused.
Surface: foundation.rs DiscoveryConfig, lib.rs:67-74 env keys.
Evidence: m80/docs/behaviors/configuration/ + m80/<crate>/tests/configuration/." \
  --silent)
emit L1_12 "$L1_12"

L2_12_1=$(br create "Env var schema" --type epic --priority 1 --parent "$L1_12" \
  --labels "$ACTIVE,configuration,env" \
  --description "Contract: M80_FIRECRACKER_BIN, M80_JAILER_BIN, M80_KERNEL_IMAGE, M80_ROOTFS_IMAGE, M80_RUN_ROOT, M80_MAX_CONCURRENT_VMS, M80_VERSION; M80_JAILER_MODE, M80_CGROUP_MODE switch sub-mode behavior.
Surface: foundation.rs env var resolution.
Evidence: m80/docs/behaviors/configuration/env-schema.md + m80/<crate>/tests/configuration/env_schema.rs." \
  --silent)
emit L2_12_1 "$L2_12_1"

L2_12_2=$(br create "Config loading order" --type epic --priority 1 --parent "$L1_12" \
  --labels "$ACTIVE,configuration,loading" \
  --description "Contract: loading precedence is built-in defaults → /etc/m80/config.toml → ~/.config/m80/config.toml → env vars → CLI flags; later values override earlier; m80 config show reveals effective config after merging.
Surface: 09-cli-shape.md Configuration loading order.
Evidence: m80/docs/behaviors/configuration/loading-order.md + m80/<crate>/tests/configuration/loading_order.rs." \
  --silent)
emit L2_12_2 "$L2_12_2"

L2_12_3=$(br create "Backend construction" --type epic --priority 1 --parent "$L1_12" \
  --labels "$ACTIVE,configuration,construction" \
  --description "Contract: FirecrackerBackendConfig::from_discovery_with_modes() built from DiscoveryConfig; singleton pattern — backend built once at service startup, reused for all requests; AdmissionLimitedSandboxBackend semaphore wraps it.
Surface: backend.rs from_discovery_with_modes.
Evidence: m80/docs/behaviors/configuration/construction.md + m80/<crate>/tests/configuration/construction.rs." \
  --silent)
emit L2_12_3 "$L2_12_3"

############################################################
# L1-13 Errors & Failure Surfaces
############################################################
L1_13=$(br create "Errors & Failure Surfaces" --type epic --priority 1 \
  --labels "$ACTIVE,errors" \
  --description "Purpose: typed VM-lifecycle errors with actionable hints. Drops predecessor-shaped variants (~350 LOC of agent-specific errors).
Contract: every error fail-closed; no silent degradation; CLI-renderable hints; machine-readable JSON envelope.
Surface: errors.rs (~600 LOC portable subset).
Evidence: m80/docs/behaviors/errors/ + m80/<crate>/tests/errors/." \
  --silent)
emit L1_13 "$L1_13"

L2_13_1=$(br create "Preflight error variants" --type epic --priority 1 --parent "$L1_13" \
  --labels "$ACTIVE,errors,preflight" \
  --description "Contract: typed variants FirecrackerBinaryNotFound, KvmUnavailable, UnsupportedHostPlatform, UnsupportedFirstLineVmSizing, InsufficientRunRootCapacity, CgroupRequiresJailer, PrivilegedJailerLaunchUnavailable; each carries actionable 'what to try next' hints; fail-closed.
Surface: errors.rs Portable preflight variants.
Evidence: m80/docs/behaviors/errors/preflight.md + m80/<crate>/tests/errors/preflight.rs." \
  --silent)
emit L2_13_1 "$L2_13_1"

L2_13_2=$(br create "Lifecycle error variants" --type epic --priority 1 --parent "$L1_13" \
  --labels "$ACTIVE,errors,lifecycle" \
  --description "Contract: typed variants BootSourceWriteFailed, MachineConfigWriteFailed for client-side API errors; VsockNotReady when ready-marker probe times out; IpCommandFailed, IptablesCommandFailed for privileged shell-out failures; UnsupportedSnapshotLaunchMode until v0.2.
Surface: errors.rs Portable lifecycle variants.
Evidence: m80/docs/behaviors/errors/lifecycle.md + m80/<crate>/tests/errors/lifecycle.rs." \
  --silent)
emit L2_13_2 "$L2_13_2"

L2_13_3=$(br create "Error to CLI mapping" --type epic --priority 1 --parent "$L1_13" \
  --labels "$ACTIVE,errors,cli" \
  --description "Contract: each error maps to a stable exit code; m80 ... --json emits a machine-readable error envelope; non-error stderr is informational only; stable variant names for downstream handlers.
Surface: errors.rs CLI mapping.
Evidence: m80/docs/behaviors/errors/cli-mapping.md + m80/<crate>/tests/errors/cli_mapping.rs." \
  --silent)
emit L2_13_3 "$L2_13_3"

############################################################
# L1-14 Concurrency, Admission, Run-Root Hygiene
############################################################
L1_14=$(br create "Concurrency, Admission & Run-Root Hygiene" --type epic --priority 1 \
  --labels "$ACTIVE,concurrency" \
  --description "Purpose: how concurrent VMs and stale state are bounded.
Contract: admission-limited via semaphore; stale state reaped by background recovery loop; ambiguous state preserves residue; cross-process collision avoided by sha256-of-path.
Surface: 05-consumers Direct consumers, lifecycle.rs run-root recovery, probe.rs.
Evidence: m80/docs/behaviors/concurrency/ + m80/<crate>/tests/concurrency/." \
  --silent)
emit L1_14 "$L1_14"

L2_14_1=$(br create "Admission limiting" --type epic --priority 1 --parent "$L1_14" \
  --labels "$ACTIVE,concurrency,admission" \
  --description "Contract: AdmissionLimitedSandboxBackend wraps backend with a semaphore; M80_FIRECRACKER_MAX_CONCURRENT_VMS bounds concurrent VMs; admission is single-host scope; multi-host placement is the caller's concern.
Surface: 05-consumers AdmissionLimitedSandboxBackend.
Evidence: m80/docs/behaviors/concurrency/admission.md + m80/<crate>/tests/concurrency/admission.rs." \
  --silent)
emit L2_14_1 "$L2_14_1"

L2_14_2=$(br create "Run-root recovery loop" --type epic --priority 1 --parent "$L1_14" \
  --labels "$ACTIVE,concurrency,recovery-loop" \
  --description "Contract: start_firecracker_run_root_recovery_loop() runs every 5 seconds; calls recover_stale_run_root() to reap orphans; concurrent-safe with active VMs; launched once at backend startup.
Surface: lifecycle.rs:1477.
Evidence: m80/docs/behaviors/concurrency/recovery-loop.md + m80/<crate>/tests/concurrency/recovery_loop.rs." \
  --silent)
emit L2_14_2 "$L2_14_2"

L2_14_3=$(br create "Stale-VM detection" --type epic --priority 1 --parent "$L1_14" \
  --labels "$ACTIVE,concurrency,detection" \
  --description "Contract: per-VM ownership markers identify stale processes; lease files checked alongside socket reachability; vsock and API-socket reachability probed; indeterminate state preserved (only confidently-dead VMs reaped).
Surface: probe.rs.
Evidence: m80/docs/behaviors/concurrency/stale-detection.md + m80/<crate>/tests/concurrency/stale_detection.rs." \
  --silent)
emit L2_14_3 "$L2_14_3"

L2_14_4=$(br create "Run-root layout invariants" --type epic --priority 1 --parent "$L1_14" \
  --labels "$ACTIVE,concurrency,layout" \
  --description "Contract: all per-VM state lives under <run_root>/<vm_id>/; cross-VM coordination (bridge ownership, IP collision) goes via run-root sibling scans; two concurrent m80 processes on the same host produce distinct run-roots; cross-process collision avoidance is via sha256-of-path.
Surface: lifecycle.rs run-root layout.
Evidence: m80/docs/behaviors/concurrency/layout.md + m80/<crate>/tests/concurrency/layout.rs." \
  --silent)
emit L2_14_4 "$L2_14_4"

############################################################
# L1-15 Cleanup, Drain, Teardown
############################################################
L1_15=$(br create "Cleanup, Drain, Teardown" --type epic --priority 1 \
  --labels "$ACTIVE,cleanup" \
  --description "Purpose: ordered teardown phases with arch-sensitive stop, idempotent and ownership-aware cleanup, force-kill preservation for triage.
Contract: admission_fence → bounded_stop → optional change-extract → residue_cleanup → release; release blocked when forced kill ambiguous or cleanup failed; teardown idempotent and ownership-aware.
Surface: lifecycle.rs:1297-1382 force_stop_and_cleanup, stage-g-firecracker-teardown-placement-release-contract.md.
Evidence: m80/docs/behaviors/cleanup/ + m80/<crate>/tests/cleanup/." \
  --silent)
emit L1_15 "$L1_15"

L2_15_1=$(br create "Teardown phase order" --type epic --priority 1 --parent "$L1_15" \
  --labels "$ACTIVE,cleanup,phases" \
  --description "Contract: explicit phase order admission_fence → bounded_stop → optional change-extract → residue_cleanup → release; release blocked when forced kill ambiguous or rollback failed (8-condition non-release set, with agent-tier writeback conditions removed).
Surface: stage-g-firecracker-teardown-placement-release-contract.md §4.
Evidence: m80/docs/behaviors/cleanup/phases.md + m80/<crate>/tests/cleanup/phases.rs." \
  --silent)
emit L2_15_1 "$L2_15_1"

L2_15_2=$(br create "Arch-sensitive stop" --type epic --priority 1 --parent "$L1_15" \
  --labels "$ACTIVE,cleanup,arch" \
  --description "Contract: x86_64 attempts graceful (SendCtrlAltDel) → forced-kill fallback; aarch64 uses forced-termination without graceful (SendCtrlAltDel unsupported); SIGKILL after 30s timeout.
Surface: lifecycle.rs:1297-1308.
Evidence: m80/docs/behaviors/cleanup/arch-stop.md + m80/<crate>/tests/cleanup/arch_stop.rs." \
  --silent)
emit L2_15_2 "$L2_15_2"

L2_15_3=$(br create "Idempotent teardown" --type epic --priority 1 --parent "$L1_15" \
  --labels "$ACTIVE,cleanup,idempotent" \
  --description "Contract: repeated teardown calls safe; ownership-aware (only owned residue removed); tolerant of partial residue and missing/malformed state files; same ownership-aware cleanup runs during startup scavenging as during normal teardown.
Surface: network.rs cleanup_vm_network, lifecycle.rs delete.
Evidence: m80/docs/behaviors/cleanup/idempotent.md + m80/<crate>/tests/cleanup/idempotent.rs." \
  --silent)
emit L2_15_3 "$L2_15_3"

L2_15_4=$(br create "Force-kill preservation" --type epic --priority 1 --parent "$L1_15" \
  --labels "$ACTIVE,cleanup,preservation" \
  --description "Contract: force_stop_and_cleanup is final fallback when graceful path fails; preserves run-dir for diagnostic preservation when cleanup fails; archive-write-failed blocks release; archive-retention-prune-failed does NOT block release.
Surface: lifecycle.rs:1308-1382.
Evidence: m80/docs/behaviors/cleanup/preservation.md + m80/<crate>/tests/cleanup/preservation.rs." \
  --silent)
emit L2_15_4 "$L2_15_4"

############################################################
# L1-16 Snapshot / Restore (mixed: schemas active, execution deferred)
############################################################
L1_16=$(br create "Snapshot / Restore (schemas active; execution deferred)" --type epic --priority 1 \
  --labels "$DEFERRED_V02,snapshot" \
  --description "Purpose: snapshot-manifest.json and restore-metadata.json schemas are obligated even in v0.1 per stage-g-firecracker-snapshot-{persistence,restore}-contract.md (refuting dossier 'drop entirely' claim per Agent 2 §J). Execution lane (pause-then-capture, restore-from-store) is deferred to v0.2.
Contract: 5-element artifact set (vmstate/memory/runtime-rootfs/scratch/boot-identity); fresh VM identity on restore; persistence path <store>/<workspace_id>/<run_id>/<unix_ms>-<sha>/; collision fail-closed.
Surface: snapshot.rs:55,73 (schemas only); stage-g-firecracker-snapshot-{persistence,restore}-contract.md.
Evidence: m80/docs/behaviors/snapshot/ + m80/<crate>/tests/snapshot/." \
  --silent)
emit L1_16 "$L1_16"

L2_16_1=$(br create "Manifest schemas (active)" --type epic --priority 1 --parent "$L1_16" \
  --labels "$ACTIVE,snapshot,schema" \
  --description "Contract: file names snapshot-manifest.json / restore-metadata.json reserved in run-root whether or not execution is wired; FirecrackerSnapshotManifest / FirecrackerRestoreMetadata types validate; 5-element artifact set (vmstate/memory/runtime-rootfs/scratch/boot-identity); diagnostics/metrics artifacts optional.
Surface: snapshot.rs:55,73; stage-g-firecracker-snapshot-restore-contract.md §Current Boundary.
Evidence: m80/docs/behaviors/snapshot/schemas.md + m80/<crate>/tests/snapshot/schemas.rs." \
  --silent)
emit L2_16_1 "$L2_16_1"

L2_16_2=$(br create "Persistence layout (active)" --type epic --priority 1 --parent "$L1_16" \
  --labels "$ACTIVE,snapshot,persistence" \
  --description "Contract: path <store-root>/<workspace_id>/<run_id>/<created_at_unix_ms>-<artifact_set_sha256>/; host-local filesystem only (no S3/GCS/generic store); collision = fail-closed.
Surface: stage-g-firecracker-snapshot-persistence-contract.md §Contract rules 1,4,7.
Evidence: m80/docs/behaviors/snapshot/persistence.md + m80/<crate>/tests/snapshot/persistence.rs." \
  --silent)
emit L2_16_2 "$L2_16_2"

L2_16_3=$(br create "Execution lane (deferred)" --type epic --priority 1 --parent "$L1_16" \
  --status deferred \
  --labels "$DEFERRED_V02,snapshot,execution" \
  --description "Contract: snapshot creation pauses/quiesces VM before artifact capture; restore materializes into fresh VM identity (restored_vm_id != source_vm_id); never in-place resume; first slice via direct/no-jailer path only.
Surface: snapshot.rs (unwired); stage-g-firecracker-snapshot-restore-contract.md §Contract rules 2,4,6.
Evidence: m80/docs/behaviors/snapshot/execution.md + m80/<crate>/tests/snapshot/execution.rs." \
  --silent)
emit L2_16_3 "$L2_16_3"

L2_16_4=$(br create "First-line VM sizing (deferred)" --type epic --priority 1 --parent "$L1_16" \
  --status deferred \
  --labels "$DEFERRED_V02,snapshot,sizing" \
  --description "Contract: snapshot timing measured on fixed first-line VM sizing (1 vCPU, 1024 MiB RAM), not benchmark-only configs.
Surface: stage-g-firecracker-snapshot-restore-contract.md §Contract rule 11.
Evidence: m80/docs/behaviors/snapshot/sizing.md + m80/<crate>/tests/snapshot/sizing.rs." \
  --silent)
emit L2_16_4 "$L2_16_4"

############################################################
# L1-17 Warm-pool / Blank VM (deferred dead code)
############################################################
L1_17=$(br create "Warm-pool / Blank VM Allocator (deferred)" --type epic --priority 1 \
  --status deferred \
  --labels "$DEFERRED_DEAD,warm-pool" \
  --description "Purpose: capture vocabulary required by stage-g-blank-vm-reset-contract.md (refuting dossier 'drop entirely' per Agent 2 §J). Allocator is unwired in predecessor; vocabulary types are required for downstream reset semantics.
Contract: BlankVmResetDecision::Reusable | Discard; reset evidence covers 8 inputs; non-inference rule prohibits inferring reuse from liveness/handles/sockets/metrics/clean-looking-tree.
Surface: blank_pool.rs:71,122,270 (vocabulary types only); stage-g-blank-vm-reset-contract.md.
Evidence: m80/docs/behaviors/warm-pool/ (defer to v0.2+ if revisited)." \
  --silent)
emit L1_17 "$L1_17"

L2_17_1=$(br create "Reset evidence vocabulary" --type epic --priority 1 --parent "$L1_17" \
  --status deferred \
  --labels "$DEFERRED_DEAD,warm-pool,vocabulary" \
  --description "Contract: BlankVmResetDecision { Reusable, Discard }; 8-input reset evidence (ownership.json+lease, boot-identity, no-workspace-id-attached, no-run-id-attached, empty/template-equal guest workspace, clean run-root surface, clean diagnostics, post-reset guestd NDJSON probe); any failure → Discard → delete-and-recreate.
Surface: blank_pool.rs:71,122,270; stage-g-blank-vm-reset-contract.md §3 Reset Inputs.
Evidence: m80/docs/behaviors/warm-pool/vocabulary.md." \
  --silent)
emit L2_17_1 "$L2_17_1"

L2_17_2=$(br create "Non-inference rule" --type epic --priority 1 --parent "$L1_17" \
  --status deferred \
  --labels "$DEFERRED_DEAD,warm-pool,non-inference" \
  --description "Contract: MUST NOT infer reuse from liveness, process handles, socket existence, metrics presence, or clean-looking directory tree; reuse only valid via explicit reset evidence.
Surface: stage-g-blank-vm-reset-contract.md §2.
Evidence: m80/docs/behaviors/warm-pool/non-inference.md." \
  --silent)
emit L2_17_2 "$L2_17_2"

############################################################
# L1-18 Observability Tail (deferred to v0.2)
############################################################
L1_18=$(br create "Observability Tail (deferred to v0.2)" --type epic --priority 1 \
  --status deferred \
  --labels "$DEFERRED_V02,observability" \
  --description "Purpose: VM-lifecycle event log + per-VM probe + health/readiness aggregation + Prometheus scrape rendering. Generic — drops Stage-H semantic agent events (sandbox_exec_*) and four-mode escalation (those are agent-tier).
Contract: structured events appended to per-run-dir diagnostics.jsonl; probe walks run-root and classifies VMs from host-visible truth only (NOT logs/metrics presence); rendering is one-shot, no embedded HTTP server.
Surface: diagnostics.rs (~1164 LOC), probe.rs (~549), health.rs (~285), readiness.rs (~317), ops_metrics.rs (~455), scrape.rs (~729) — likely partially dropped.
Evidence: m80/docs/behaviors/observability/ (when v0.2 begins)." \
  --silent)
emit L1_18 "$L1_18"

L2_18_1=$(br create "Diagnostics event log" --type epic --priority 1 --parent "$L1_18" \
  --status deferred \
  --labels "$DEFERRED_V02,observability,diagnostics" \
  --description "Contract: structured events appended to <run_dir>/diagnostics.jsonl; phases StartupScavenge/HostPreflight/StoragePrepare/Boot/Ready/Stop/Delete; wrapped Option<VmDiagnostics> in lifecycle.rs:72 so removal doesn't break boot; triage bundles archivable for offline analysis.
Surface: diagnostics.rs.
Evidence: m80/docs/behaviors/observability/diagnostics.md (v0.2)." \
  --silent)
emit L2_18_1 "$L2_18_1"

L2_18_2=$(br create "Per-VM probe" --type epic --priority 1 --parent "$L1_18" \
  --status deferred \
  --labels "$DEFERRED_V02,observability,probe" \
  --description "Contract: walks <run_root>/*/; reads ownership markers and lease files; checks vsock and API-socket reachability; peeks at last failure kind; emits FirecrackerVmProbeRecord with health classification (healthy/degraded/stuck/exited).
Surface: probe.rs.
Evidence: m80/docs/behaviors/observability/probe.md (v0.2)." \
  --silent)
emit L2_18_2 "$L2_18_2"

L2_18_3=$(br create "Health and readiness aggregation" --type epic --priority 1 --parent "$L1_18" \
  --status deferred \
  --labels "$DEFERRED_V02,observability,health" \
  --description "Contract: health.rs maps probe records to workspace health views with ready/stuck flags; readiness.rs summarizes health into rollout-readiness counts; ops_metrics.rs aggregates per-VM metrics.json into bounded latency families.
Surface: health.rs, readiness.rs, ops_metrics.rs.
Evidence: m80/docs/behaviors/observability/health.md (v0.2)." \
  --silent)
emit L2_18_3 "$L2_18_3"

L2_18_4=$(br create "Prometheus scrape" --type epic --priority 1 --parent "$L1_18" \
  --status deferred \
  --labels "$DEFERRED_V02,observability,prometheus" \
  --description "Contract: scrape.rs::render_prometheus_metrics renders Prometheus text format from ops-metrics + health; renders JSON health snapshot; rendering only — no embedded HTTP server.
Surface: scrape.rs.
Evidence: m80/docs/behaviors/observability/prometheus.md (v0.2)." \
  --silent)
emit L2_18_4 "$L2_18_4"

############################################################
# Summary
############################################################
echo "Skeleton created. IDs persisted in $ID_FILE"
br stats
