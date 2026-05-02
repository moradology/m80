# Leaves: Platform L1s (Storage, Image Build, Jailer, Cgroup, NoEgress)

This file enumerates leaf beads for five m80 L1 epics. Each leaf cites a
predecessor source location and an m80 capture pair (contract doc + test).

Leaves carry the `$ACTIVE` label plus a behavior-domain tag. m80 captures
generic VM-sandbox behaviors: change-extraction is opt-in (caller-driven),
not gated on any agent-tier `EffectClass`. The image build does not
opine on interpreter packaging.

---

# L1-03 Storage & Filesystem (parent_var: $L1_03)

## L2-03.1 Per-VM rootfs cloning (parent_var: $L2_03_1)

### Leaf: Clone managed rootfs into per-VM runtime image
- parent_var: $L2_03_1
- labels: $ACTIVE,storage,rootfs
- status: open
- behavior: The system copies the managed base rootfs ext4 image into a per-VM `runtime_rootfs` path under the VM run-dir before each boot.
- source: dossier `07-modules-essential-vs-hygiene.md` § storage.rs; predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `prepare_runtime_rootfs_clone` lines 117-135.
- captured-by: m80/docs/behaviors/storage/per-vm-rootfs-clone.md#clone-base + m80/m80-storage/tests/storage/per_vm_rootfs_clone.rs::clones_managed_rootfs

### Leaf: Create runtime rootfs parent directories before copy
- parent_var: $L2_03_1
- labels: $ACTIVE,storage,rootfs
- status: open
- behavior: The system creates any missing parent directories of the runtime rootfs path before copying the managed image into it.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `prepare_runtime_rootfs_clone` lines 121-126.
- captured-by: m80/docs/behaviors/storage/per-vm-rootfs-clone.md#parents + m80/m80-storage/tests/storage/per_vm_rootfs_clone.rs::creates_run_dir_parents

### Leaf: Verify managed rootfs boot identity before clone
- parent_var: $L2_03_1
- labels: $ACTIVE,storage,rootfs,boot-identity
- status: open
- behavior: The system verifies the managed rootfs and kernel boot identity (sha256 against the manifest) before cloning, and writes the verified identity to the per-VM run-dir.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `prepare_vm_storage` lines 36-52.
- captured-by: m80/docs/behaviors/storage/per-vm-rootfs-clone.md#verify-identity + m80/m80-storage/tests/storage/per_vm_rootfs_clone.rs::verifies_boot_identity

### Leaf: Surface CopyRootfs error with both paths on clone failure
- parent_var: $L2_03_1
- labels: $ACTIVE,storage,rootfs,errors
- status: open
- behavior: The system maps a failed `fs::copy` of the managed rootfs to a typed `CopyRootfs` error carrying both the source and destination paths.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `prepare_runtime_rootfs_clone` lines 127-133.
- captured-by: m80/docs/behaviors/storage/per-vm-rootfs-clone.md#copy-error + m80/m80-storage/tests/storage/per_vm_rootfs_clone.rs::copy_error_carries_paths

## L2-03.2 Scratch workspace image (parent_var: $L2_03_2)

### Leaf: Build scratch ext4 image via mkfs.ext4 with workspace label
- parent_var: $L2_03_2
- labels: $ACTIVE,storage,scratch
- status: open
- behavior: The system creates the per-VM scratch ext4 image using `mkfs.ext4 -F -L <DEFAULT_WORKSPACE_DRIVE_LABEL> -d <workspace>` so that guest tooling can resolve it by label.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `prepare_workspace_scratch` lines 171-183.
- captured-by: m80/docs/behaviors/storage/scratch-image.md#mkfs + m80/m80-storage/tests/storage/scratch_image.rs::mkfs_uses_workspace_label

### Leaf: Hydrate scratch image from host workspace tree at creation
- parent_var: $L2_03_2
- labels: $ACTIVE,storage,scratch
- status: open
- behavior: The system populates the freshly created scratch image with the host workspace tree by passing `-d <host_workspace_root>` to `mkfs.ext4` so the guest sees the workspace at first mount.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `prepare_workspace_scratch` lines 178-181.
- captured-by: m80/docs/behaviors/storage/scratch-image.md#hydrate + m80/m80-storage/tests/storage/scratch_image.rs::hydrates_from_host_workspace

### Leaf: Size scratch image with padding and 4 MiB alignment
- parent_var: $L2_03_2
- labels: $ACTIVE,storage,scratch,sizing
- status: open
- behavior: The system sizes the scratch image as `max(64 MiB, used_bytes + 32 MiB)` rounded up to a 4 MiB boundary, sufficient for expected guest mutations.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` constants `MIN_SCRATCH_BYTES` `SCRATCH_PADDING_BYTES` `SCRATCH_ALIGNMENT_BYTES` lines 16-18; `aligned_scratch_size` lines 688-694.
- captured-by: m80/docs/behaviors/storage/scratch-image.md#sizing + m80/m80-storage/tests/storage/scratch_image.rs::sizing_obeys_padding_and_alignment

### Leaf: Pre-allocate scratch image file via set_len before format
- parent_var: $L2_03_2
- labels: $ACTIVE,storage,scratch
- status: open
- behavior: The system opens the scratch image for create+truncate+write, sets its length to the planned size, and only then runs `mkfs.ext4` against the file.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `prepare_workspace_scratch` lines 154-170.
- captured-by: m80/docs/behaviors/storage/scratch-image.md#preallocate + m80/m80-storage/tests/storage/scratch_image.rs::preallocates_before_format

### Leaf: Reject inadmissible inodes in host workspace before hydration
- parent_var: $L2_03_2
- labels: $ACTIVE,storage,scratch,admissibility
- status: open
- behavior: The system walks the host workspace before scratch creation and rejects symlinks, sockets, FIFOs, device nodes, and hardlinks via `WorkspaceEntryRejected`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `validate_workspace_tree_admissibility` lines 481-516; `validate_workspace_entry` lines 518-568.
- captured-by: m80/docs/behaviors/storage/scratch-image.md#host-admissibility + m80/m80-storage/tests/storage/scratch_image.rs::rejects_inadmissible_host_entries

## L2-03.3 Change extraction (opt-in) (parent_var: $L2_03_3)

### Leaf: Run change extraction only when caller opts in
- parent_var: $L2_03_3
- labels: $ACTIVE,storage,extract,opt-in
- status: open
- behavior: The system performs post-stop change extraction only when the caller requests it; absent that request the scratch image is left untouched for the caller to discard or archive.
- source: dossier `01-coupling-audit.md` § "writeback model"/"Decoupling it"; dossier `07-modules-essential-vs-hygiene.md` § storage.rs.
- captured-by: m80/docs/behaviors/storage/change-extraction.md#opt-in + m80/m80-storage/tests/storage/change_extraction.rs::extraction_only_when_requested

### Leaf: Repair scratch image journal with e2fsck before reads
- parent_var: $L2_03_3
- labels: $ACTIVE,storage,extract,e2fsck
- status: open
- behavior: The system runs `e2fsck -fy` on the scratch image after VM stop and treats only exit codes whose top bits are zero (`code & !0b11 == 0`) as success.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `repair_workspace_image` lines 200-223; `e2fsck_exit_code_is_acceptable` lines 225-227.
- captured-by: m80/docs/behaviors/storage/change-extraction.md#e2fsck + m80/m80-storage/tests/storage/change_extraction.rs::e2fsck_exit_status_classification

### Leaf: Extract changed file set with debugfs rdump
- parent_var: $L2_03_3
- labels: $ACTIVE,storage,extract,debugfs
- status: open
- behavior: The system extracts the modified file set out of the scratch image into a temp directory by invoking `debugfs -R "rdump / <extract_root>"` against the image.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `extract_workspace_image` lines 186-198.
- captured-by: m80/docs/behaviors/storage/change-extraction.md#debugfs-rdump + m80/m80-storage/tests/storage/change_extraction.rs::debugfs_rdump_invocation

### Leaf: Build a writeback staging tree before swap
- parent_var: $L2_03_3
- labels: $ACTIVE,storage,extract,staging
- status: open
- behavior: The system materializes the extracted tree into a sibling staging directory (named `.<workspace>.m80-writeback-stage-<pid>-<n>`) before any swap, isolating the in-progress result from the live workspace.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `create_workspace_stage_dir` lines 384-411; `sanitize_workspace_tree` lines 413-475.
- captured-by: m80/docs/behaviors/storage/change-extraction.md#staging + m80/m80-storage/tests/storage/change_extraction.rs::stages_into_sibling_directory

### Leaf: Roll back to original workspace on extraction failure
- parent_var: $L2_03_3
- labels: $ACTIVE,storage,extract,rollback
- status: open
- behavior: The system tears down the staging tree and surfaces the original error if extraction or sanitisation fails, leaving the host workspace untouched.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `write_back_workspace_with_stage_renamer` lines 71-109; `cleanup_stage_root` lines 570-578.
- captured-by: m80/docs/behaviors/storage/change-extraction.md#rollback + m80/m80-storage/tests/storage/change_extraction.rs::rollback_on_extract_failure

## L2-03.4 Admissibility and atomic swap (parent_var: $L2_03_4)

### Leaf: Admit only regular files and directories from scratch image
- parent_var: $L2_03_4
- labels: $ACTIVE,storage,admissibility
- status: open
- behavior: The system scans the scratch image inode tree via `debugfs ls -p` and rejects symlinks, FIFOs, sockets, device nodes, and hardlinked regular files, allowing only ordinary files and directories to survive extraction.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `scan_workspace_image_admissibility` lines 229-314; `parse_debugfs_ls_entry` lines 342-369.
- captured-by: m80/docs/behaviors/storage/admissibility.md#scan + m80/m80-storage/tests/storage/admissibility.rs::rejects_non_regular_inodes

### Leaf: Atomically swap staged tree into host workspace with backup
- parent_var: $L2_03_4
- labels: $ACTIVE,storage,swap,atomic
- status: open
- behavior: The system renames the existing host workspace aside as a backup, renames the staged tree into place, and restores the backup if the final rename fails so the live tree is never partially overwritten.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `swap_staged_workspace_with_stage_renamer` lines 595-686.
- captured-by: m80/docs/behaviors/storage/admissibility.md#atomic-swap + m80/m80-storage/tests/storage/admissibility.rs::atomic_swap_with_rollback

---

# L1-07 Image Build Pipeline (parent_var: $L1_07)

## L2-07.1 Source artifact acquisition (parent_var: $L2_07_1)

### Leaf: Resolve kernel from firecracker-ci artifacts directory
- parent_var: $L2_07_1
- labels: $ACTIVE,image-build,kernel
- status: open
- behavior: The system resolves the kernel image by either honoring `FIRECRACKER_KERNEL_IMAGE` or finding the highest-versioned `vmlinux-*` under `/opt/firecracker/artifacts`.
- source: dossier `04-infra-and-artifacts.md` § Kernel; predecessor `infra/firecracker/prepare-guestd-image.sh` lines 24, 41-43.
- captured-by: m80/docs/behaviors/image-build/source-artifacts.md#kernel-discovery + m80/m80-image-build/tests/image_build/source_artifacts.rs::resolves_kernel_path

### Leaf: Resolve source rootfs from firecracker-ci squashfs/ext4 drop
- parent_var: $L2_07_1
- labels: $ACTIVE,image-build,rootfs
- status: open
- behavior: The system resolves the source rootfs image by either honoring `FIRECRACKER_ROOTFS_IMAGE` or selecting the highest-versioned non-output `*.ext4` under `/opt/firecracker/artifacts`.
- source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 8, 45-52.
- captured-by: m80/docs/behaviors/image-build/source-artifacts.md#rootfs-discovery + m80/m80-image-build/tests/image_build/source_artifacts.rs::resolves_source_rootfs

### Leaf: Fail-closed when kernel or source rootfs is missing
- parent_var: $L2_07_1
- labels: $ACTIVE,image-build,fail-closed
- status: open
- behavior: The system exits non-zero with an explicit error message when either the kernel image or the source rootfs cannot be located on disk.
- source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 49-57.
- captured-by: m80/docs/behaviors/image-build/source-artifacts.md#missing-artifact + m80/m80-image-build/tests/image_build/source_artifacts.rs::missing_artifact_fails_closed

### Leaf: Probe firecracker binary for version when not specified
- parent_var: $L2_07_1
- labels: $ACTIVE,image-build,version
- status: open
- behavior: The system records the expected Firecracker version by either trusting `FIRECRACKER_VERSION` or parsing `firecracker --version` output.
- source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 23, 59-62.
- captured-by: m80/docs/behaviors/image-build/source-artifacts.md#firecracker-version + m80/m80-image-build/tests/image_build/source_artifacts.rs::probes_firecracker_version

### Leaf: Resize source rootfs ext4 image to target size
- parent_var: $L2_07_1
- labels: $ACTIVE,image-build,rootfs,sizing
- status: open
- behavior: The system makes the source rootfs writable by copying it to the output path (`truncate`-grown to the configured target size) before any chroot edits.
- source: dossier `04-infra-and-artifacts.md` § Rootfs Pipeline; predecessor `infra/firecracker/prepare-guestd-image.sh` lines 199-201.
- captured-by: m80/docs/behaviors/image-build/source-artifacts.md#resize + m80/m80-image-build/tests/image_build/source_artifacts.rs::resizes_output_rootfs

## L2-07.2 Chroot customization (parent_var: $L2_07_2)

### Leaf: Mount output rootfs read-write via loop device
- parent_var: $L2_07_2
- labels: $ACTIVE,image-build,chroot
- status: open
- behavior: The system mounts the output rootfs ext4 image read-write via a loop device into a temp directory before installing daemon assets.
- source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 199-201.
- captured-by: m80/docs/behaviors/image-build/chroot-customization.md#loop-mount + m80/m80-image-build/tests/image_build/chroot_customization.rs::loop_mounts_output_rootfs

### Leaf: Install m80 guest daemon binary into rootfs at canonical path
- parent_var: $L2_07_2
- labels: $ACTIVE,image-build,chroot,daemon
- status: open
- behavior: The system installs the freshly built guest daemon binary into the rootfs at `/usr/local/bin/<daemon>` with mode 0755.
- source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 18, 204.
- captured-by: m80/docs/behaviors/image-build/chroot-customization.md#install-daemon + m80/m80-image-build/tests/image_build/chroot_customization.rs::installs_daemon_binary

### Leaf: Install daemon systemd unit and workspace mount unit
- parent_var: $L2_07_2
- labels: $ACTIVE,image-build,chroot,systemd
- status: open
- behavior: The system installs the daemon's systemd service unit and the workspace `.mount` unit into `/etc/systemd/system/` and links them into `multi-user.target.wants/` so they start on boot.
- source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 19-21, 205-206, 218-221.
- captured-by: m80/docs/behaviors/image-build/chroot-customization.md#systemd-units + m80/m80-image-build/tests/image_build/chroot_customization.rs::installs_systemd_units

## L2-07.3 Provenance manifest (parent_var: $L2_07_3)

### Leaf: Emit provenance manifest beside the output rootfs
- parent_var: $L2_07_3
- labels: $ACTIVE,image-build,manifest
- status: open
- behavior: The system writes `<output_rootfs>.manifest.json` alongside the output rootfs at the end of the build with mode 0644.
- source: dossier `04-infra-and-artifacts.md` § Provenance manifest; predecessor `infra/firecracker/prepare-guestd-image.sh` lines 10, 242-316.
- captured-by: m80/docs/behaviors/image-build/manifest.md#emit + m80/m80-image-build/tests/image_build/manifest.rs::emits_manifest_beside_rootfs

### Leaf: Pin manifest with schema_version and expected_firecracker_version
- parent_var: $L2_07_3
- labels: $ACTIVE,image-build,manifest,schema
- status: open
- behavior: The system stamps the manifest with `schema_version: 1` and the resolved `expected_firecracker_version` so a boot-time validator can refuse mismatches.
- source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 22-23, 246-247.
- captured-by: m80/docs/behaviors/image-build/manifest.md#schema-version + m80/m80-image-build/tests/image_build/manifest.rs::stamps_schema_and_firecracker_version

### Leaf: Record sha256 over kernel, source rootfs, output rootfs, daemon, and units
- parent_var: $L2_07_3
- labels: $ACTIVE,image-build,manifest,sha256
- status: open
- behavior: The system records `sha256sum` digests for the kernel, source rootfs, output rootfs, daemon binary, service unit, and workspace mount unit in the manifest.
- source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 225-233, 248-265.
- captured-by: m80/docs/behaviors/image-build/manifest.md#sha256-coverage + m80/m80-image-build/tests/image_build/manifest.rs::sha256_covers_all_inputs

### Leaf: Record boot_target, guest_port, and ready_marker in manifest
- parent_var: $L2_07_3
- labels: $ACTIVE,image-build,manifest,boot
- status: open
- behavior: The system records the configured `boot_target`, `guest_port`, and `ready_marker` in the manifest so the host can derive vsock and serial-probe parameters at boot.
- source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 15-17, 308-310.
- captured-by: m80/docs/behaviors/image-build/manifest.md#boot-fields + m80/m80-image-build/tests/image_build/manifest.rs::records_boot_target_port_marker

## L2-07.4 Manifest verification at boot (parent_var: $L2_07_4)

### Leaf: Validate manifest schema before each boot
- parent_var: $L2_07_4
- labels: $ACTIVE,image-build,manifest,verify
- status: open
- behavior: The system rejects boot when the manifest fails schema validation (missing required fields or incompatible `schema_version`).
- source: dossier `04-infra-and-artifacts.md` § "Host preflight" check 8.
- captured-by: m80/docs/behaviors/image-build/manifest-verify.md#schema-check + m80/m80-image-build/tests/image_build/manifest_verify.rs::rejects_invalid_schema

### Leaf: Recompute sha256 of artifacts and refuse on tampering
- parent_var: $L2_07_4
- labels: $ACTIVE,image-build,manifest,verify,sha256
- status: open
- behavior: The system recomputes the sha256 of the kernel, output rootfs, and daemon at boot and refuses to boot when any digest differs from the manifest record.
- source: dossier `04-infra-and-artifacts.md` § "Why this is good" + "m80 manifest changes"; predecessor `crates/sandbox/agent-sandbox-firecracker/src/storage.rs` `verify_boot_identity` (called from `prepare_vm_storage` lines 42-43).
- captured-by: m80/docs/behaviors/image-build/manifest-verify.md#sha256-recompute + m80/m80-image-build/tests/image_build/manifest_verify.rs::tampered_sha_refused

---

# L1-08 Jailer & Privilege Drop (parent_var: $L1_08)

## L2-08.1 Jail root layout (parent_var: $L2_08_1)

### Leaf: Materialize per-VM jail root under run-dir/jail-root
- parent_var: $L2_08_1
- labels: $ACTIVE,jailer,layout
- status: open
- behavior: The system places each VM's jail root at `<run_dir>/jail-root` named exactly `JAIL_ROOT_DIR`, validated by the plan record.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs` `JAIL_ROOT_DIR` line 44; predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` lines 767, 783-786.
- captured-by: m80/docs/behaviors/jailer/jail-root-layout.md#per-vm-jail-root + m80/m80-jailer/tests/jailer/jail_root_layout.rs::jail_root_under_run_dir

### Leaf: Anchor jailer chroot base under configurable system path
- parent_var: $L2_08_1
- labels: $ACTIVE,jailer,layout,chroot-base
- status: open
- behavior: The system probes a configurable jailer chroot base (renamed from `/var/tmp/predecessor-fc-jailer` to an m80-prefixed path) and falls back to a per-run-root sibling directory when the base is not writable.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `OFFICIAL_JAILER_BASE_ROOT` line 36; `projected_jailer_chroot_base_dir_with` lines 470-504.
- captured-by: m80/docs/behaviors/jailer/jail-root-layout.md#chroot-base + m80/m80-jailer/tests/jailer/jail_root_layout.rs::probes_official_then_falls_back

### Leaf: Persist prepared jailer plan as jailer-plan.json
- parent_var: $L2_08_1
- labels: $ACTIVE,jailer,layout,plan
- status: open
- behavior: The system serializes the `PreparedJailerPlan` to `<run_dir>/jailer-plan.json` via an atomic write, so plan recovery is replayable.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs` `JAILER_PLAN_FILE` line 45; predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `write_prepared_jailer_plan` lines 341-358.
- captured-by: m80/docs/behaviors/jailer/jail-root-layout.md#plan-persisted + m80/m80-jailer/tests/jailer/jail_root_layout.rs::plan_atomically_written

### Leaf: Persist jailer runtime state as jailer-state.json
- parent_var: $L2_08_1
- labels: $ACTIVE,jailer,layout,state
- status: open
- behavior: The system writes `<run_dir>/jailer-state.json` carrying the runtime phase (Planned, Materialized, Running, Stopped, Cleaned, CleanupFailed) and the captured pids.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs` `JAILER_STATE_FILE` line 46; predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `JailerRuntimePhase` lines 102-110; `write_jailer_runtime_state` lines 397-414.
- captured-by: m80/docs/behaviors/jailer/jail-root-layout.md#state-persisted + m80/m80-jailer/tests/jailer/jail_root_layout.rs::runtime_state_phases

## L2-08.2 Asset binding plan (parent_var: $L2_08_2)

### Leaf: Bind kernel and firecracker binary read-only into jail
- parent_var: $L2_08_2
- labels: $ACTIVE,jailer,binding,readonly
- status: open
- behavior: The system exposes the firecracker binary and kernel image to the jail as `BindRo` mounts at `bin/firecracker` and `kernel/vmlinux`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `JAILER_BIN_PATH` line 30, `JAILER_KERNEL_PATH` line 31; `build_jailer_plan` asset list lines 288-296.
- captured-by: m80/docs/behaviors/jailer/asset-binding.md#kernel-bin-ro + m80/m80-jailer/tests/jailer/asset_binding.rs::binds_kernel_and_firecracker_ro

### Leaf: Bind runtime rootfs and workspace scratch read-write into jail
- parent_var: $L2_08_2
- labels: $ACTIVE,jailer,binding,readwrite
- status: open
- behavior: The system exposes the per-VM runtime rootfs and workspace scratch images to the jail as `BindRw` mounts at `drives/rootfs.ext4` and `drives/workspace.ext4`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `JAILER_ROOTFS_PATH` line 32, `JAILER_WORKSPACE_PATH` line 33; `build_jailer_plan` lines 297-304.
- captured-by: m80/docs/behaviors/jailer/asset-binding.md#drives-rw + m80/m80-jailer/tests/jailer/asset_binding.rs::binds_drives_rw

### Leaf: Allocate API and vsock sockets inside the jail
- parent_var: $L2_08_2
- labels: $ACTIVE,jailer,binding,sockets
- status: open
- behavior: The system declares both the Firecracker API socket and the vsock UDS as `CreateInsideJail` entries at `run/firecracker.sock` and `run/vsock.sock`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `JAILER_API_SOCKET_PATH` line 34, `JAILER_VSOCK_SOCKET_PATH` line 35; `build_jailer_plan` lines 305-310.
- captured-by: m80/docs/behaviors/jailer/asset-binding.md#sockets-inside + m80/m80-jailer/tests/jailer/asset_binding.rs::sockets_created_inside_jail

### Leaf: Keep ownership marker and lease as host-only artifacts
- parent_var: $L2_08_2
- labels: $ACTIVE,jailer,binding,host-only
- status: open
- behavior: The system declares ownership markers, lease files, verified boot identity, console log, diagnostics log, and metrics snapshot as `HostOnly` so they live outside the jail tree.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `build_jailer_plan` lines 311-334.
- captured-by: m80/docs/behaviors/jailer/asset-binding.md#host-only-artifacts + m80/m80-jailer/tests/jailer/asset_binding.rs::host_only_artifacts_listed

### Leaf: Compute asset binding plan before launch and replay deterministically
- parent_var: $L2_08_2
- labels: $ACTIVE,jailer,binding,replay
- status: open
- behavior: The system computes the full asset exposure list pre-launch via `build_jailer_plan`, validates it, and persists it so subsequent recovery uses the identical plan.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `build_jailer_plan` lines 264-339; `read_prepared_jailer_plan` lines 360-373.
- captured-by: m80/docs/behaviors/jailer/asset-binding.md#replayable + m80/m80-jailer/tests/jailer/asset_binding.rs::plan_round_trips_via_disk

## L2-08.3 Privilege drop (parent_var: $L2_08_3)

### Leaf: Make jailed UID and GID configurable (default 3000:3000)
- parent_var: $L2_08_3
- labels: $ACTIVE,jailer,privilege,uid-gid
- status: open
- behavior: The system runs the jailed firecracker process as a configurable UID/GID pair, defaulting to 3000:3000 to match the upstream predecessor fixed-id values.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `JAILER_UID`/`JAILER_GID` lines 25-26; dossier `07-modules-essential-vs-hygiene.md` § jailer.rs.
- captured-by: m80/docs/behaviors/jailer/privilege-drop.md#configurable-uid-gid + m80/m80-jailer/tests/jailer/privilege_drop.rs::uid_gid_configurable

### Leaf: Track jailer_pid and firecracker_pid in runtime state
- parent_var: $L2_08_3
- labels: $ACTIVE,jailer,privilege,pids
- status: open
- behavior: The system records both the wrapping `jailer_pid` and the inner `firecracker_pid` in `JailerRuntimeState` and asserts the `Running` phase requires both.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `JailerRuntimeState` lines 112-120; phase validation lines 218-249.
- captured-by: m80/docs/behaviors/jailer/privilege-drop.md#track-pids + m80/m80-jailer/tests/jailer/privilege_drop.rs::running_requires_both_pids

### Leaf: Verify launch privilege at startup once
- parent_var: $L2_08_3
- labels: $ACTIVE,jailer,privilege,startup
- status: open
- behavior: The system checks that the process is effective root or has passwordless sudo at startup and surfaces `PrivilegedJailerLaunchUnavailable` when neither is true.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs` `verify_jailer_launch_privilege` lines 847-853.
- captured-by: m80/docs/behaviors/jailer/privilege-drop.md#startup-check + m80/m80-jailer/tests/jailer/privilege_drop.rs::startup_privilege_required

### Leaf: Surface PrivilegedJailerLaunchUnavailable as typed error
- parent_var: $L2_08_3
- labels: $ACTIVE,jailer,privilege,errors
- status: open
- behavior: The system returns a typed `PrivilegedJailerLaunchUnavailable` error rather than panicking when launch privilege is unavailable, so callers can decide how to escalate.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs` lines 850-852; dossier `07-modules-essential-vs-hygiene.md` § errors.rs (Portable variants).
- captured-by: m80/docs/behaviors/jailer/privilege-drop.md#typed-error + m80/m80-jailer/tests/jailer/privilege_drop.rs::typed_privilege_error

## L2-08.4 Scavenge on startup (parent_var: $L2_08_4)

### Leaf: Recover jailer state from run-dir on restart
- parent_var: $L2_08_4
- labels: $ACTIVE,jailer,scavenge,recovery
- status: open
- behavior: The system reconstructs `JailerRuntimeState` from `<run_dir>/jailer-plan.json` and `<run_dir>/jailer-state.json` so a restarted host can resume tracking existing jailers.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `recover_jailer_from_run_dir` lines 452-455; `recover_jailer_from_run_dir_with_ops` lines 722-727.
- captured-by: m80/docs/behaviors/jailer/scavenge.md#recover-from-run-dir + m80/m80-jailer/tests/jailer/scavenge.rs::recovers_state_from_disk

### Leaf: Identify orphan jailer processes via tracked pids
- parent_var: $L2_08_4
- labels: $ACTIVE,jailer,scavenge,orphans
- status: open
- behavior: The system probes the recorded `jailer_pid` for liveness and walks its child processes to identify the matching firecracker pid before declaring the jailer alive.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` `discover_running_jailer_state` lines 457-464; child resolution lines 1285-1363.
- captured-by: m80/docs/behaviors/jailer/scavenge.md#identify-orphans + m80/m80-jailer/tests/jailer/scavenge.rs::resolves_running_jailer_via_pids

### Leaf: Preserve residue when scavenge is ambiguous
- parent_var: $L2_08_4
- labels: $ACTIVE,jailer,scavenge,non-destructive
- status: open
- behavior: The system leaves jailer residue intact rather than deleting it whenever startup scavenge cannot conclusively classify the recorded pids as dead.
- source: dossier `07-modules-essential-vs-hygiene.md` § jailer.rs (startup scavenging); predecessor `crates/sandbox/agent-sandbox-firecracker/src/jailer.rs` lines 1347-1363.
- captured-by: m80/docs/behaviors/jailer/scavenge.md#preserve-residue + m80/m80-jailer/tests/jailer/scavenge.rs::ambiguity_preserves_residue

---

# L1-09 Cgroup v2 Limits (parent_var: $L1_09)

## L2-09.1 Subtree creation (parent_var: $L2_09_1)

### Leaf: Place per-VM cgroup under m80-firecracker root
- parent_var: $L2_09_1
- labels: $ACTIVE,cgroup,subtree
- status: open
- behavior: The system creates the per-VM cgroup leaf under `/sys/fs/cgroup/m80-firecracker/<jailer_instance_id>` (renamed from predecessor's `predecessor-firecracker` root).
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs` `DEFAULT_FIRECRACKER_CGROUP_ROOT` line 13; `unified_v2_leaf_path` lines 155-157.
- captured-by: m80/docs/behaviors/cgroup/subtree.md#root-path + m80/m80-cgroup/tests/cgroup/subtree.rs::leaf_under_renamed_root

### Leaf: Enable cpu, memory, and pids in subtree_control before leaf
- parent_var: $L2_09_1
- labels: $ACTIVE,cgroup,subtree,controllers
- status: open
- behavior: The system writes `+cpu +memory +pids` to the cgroup root's `cgroup.subtree_control` after verifying availability, before creating the per-VM leaf.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs` `materialize_jailed_cgroup` lines 84-93; `REQUIRED_CONTROLLERS` line 16.
- captured-by: m80/docs/behaviors/cgroup/subtree.md#subtree-control + m80/m80-cgroup/tests/cgroup/subtree.rs::enables_required_controllers

### Leaf: Assign jailer and firecracker pids to leaf cgroup
- parent_var: $L2_09_1
- labels: $ACTIVE,cgroup,subtree,pids
- status: open
- behavior: The system writes both the jailer pid and firecracker pid (deduplicated, sorted) into the leaf's `cgroup.procs` once the leaf is created.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs` `materialize_jailed_cgroup` lines 101-110.
- captured-by: m80/docs/behaviors/cgroup/subtree.md#pid-assign + m80/m80-cgroup/tests/cgroup/subtree.rs::pids_assigned_to_leaf

## L2-09.2 Limit enforcement (parent_var: $L2_09_2)

### Leaf: Set cpu.max to one full CPU equivalent
- parent_var: $L2_09_2
- labels: $ACTIVE,cgroup,limits,cpu
- status: open
- behavior: The system writes `cpu.max` as `100000 100000` (one full CPU period) on the leaf cgroup.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs` `CPU_MAX_VALUE` line 17; `materialize_jailed_cgroup` line 94.
- captured-by: m80/docs/behaviors/cgroup/limits.md#cpu-max + m80/m80-cgroup/tests/cgroup/limits.rs::cpu_max_one_cpu

### Leaf: Set memory.max to 1.5 GiB and pids.max to 128
- parent_var: $L2_09_2
- labels: $ACTIVE,cgroup,limits,memory,pids
- status: open
- behavior: The system writes `memory.max` as 1610612736 bytes (1.5 GiB) and `pids.max` as 128 on the leaf cgroup.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs` `MEMORY_MAX_VALUE_BYTES` line 18, `PIDS_MAX_VALUE` line 19; `materialize_jailed_cgroup` lines 95-99.
- captured-by: m80/docs/behaviors/cgroup/limits.md#memory-pids-max + m80/m80-cgroup/tests/cgroup/limits.rs::memory_and_pids_max

### Leaf: Gate enforcement on M80_CGROUP_MODE=unified-v2
- parent_var: $L2_09_2
- labels: $ACTIVE,cgroup,limits,gate
- status: open
- behavior: The system applies cgroup limits only when `M80_CGROUP_MODE` resolves to `unified-v2` and short-circuits as a no-op when set to `disabled`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs` `FirecrackerCgroupMode` lines 21-46 (renamed env key in m80); `materialize_jailed_cgroup` lines 79-81.
- captured-by: m80/docs/behaviors/cgroup/limits.md#mode-gate + m80/m80-cgroup/tests/cgroup/limits.rs::disabled_mode_skips

## L2-09.3 Failure modes (parent_var: $L2_09_3)

### Leaf: Refuse cgroup unified-v2 when jailer is not enabled
- parent_var: $L2_09_3
- labels: $ACTIVE,cgroup,failure,errors
- status: open
- behavior: The system surfaces `CgroupRequiresJailer` from preflight when cgroup mode is `unified-v2` but jailer mode is not `Enabled`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs` `verify_cgroup_host_preflight` lines 49-72.
- captured-by: m80/docs/behaviors/cgroup/failure-modes.md#requires-jailer + m80/m80-cgroup/tests/cgroup/failure_modes.rs::requires_jailer

### Leaf: Cleanup leaf and root cgroups idempotently on VM delete
- parent_var: $L2_09_3
- labels: $ACTIVE,cgroup,failure,cleanup
- status: open
- behavior: The system runs `rmdir` against the leaf cgroup, then the root cgroup, tolerating "no such file" and "directory not empty" errors so cleanup is safe to retry.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/cgroup.rs` `cleanup_materialized_cgroup` lines 127-149.
- captured-by: m80/docs/behaviors/cgroup/failure-modes.md#cleanup-idempotent + m80/m80-cgroup/tests/cgroup/failure_modes.rs::cleanup_idempotent

---

# L1-10 Networking — NoEgress (parent_var: $L1_10)

## L2-10.1 NoEgress configuration (parent_var: $L2_10_1)

### Leaf: Skip bridge, tap, and NIC wiring under NoEgress mode
- parent_var: $L2_10_1
- labels: $ACTIVE,network,noegress
- status: open
- behavior: The system returns `PreparedVmNetwork::NoEgress` without creating a bridge or tap when the resolved mode is `NoEgress`, so Firecracker's machine config carries no NIC.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `prepare_vm_network_with_host` lines 197-205; `firecracker_interfaces` lines 147-153.
- captured-by: m80/docs/behaviors/network/noegress-config.md#no-bridge-or-tap + m80/m80-network/tests/network/noegress_config.rs::no_nic_wired_under_noegress

### Leaf: Provide loopback only inside the guest under NoEgress
- parent_var: $L2_10_1
- labels: $ACTIVE,network,noegress,loopback
- status: open
- behavior: The system leaves the guest with loopback as the only available network interface when `NoEgress` is selected, since no NIC is attached at boot.
- source: dossier `06-network-internals.md` § "Recommendation for v0.1"; dossier `04-infra-and-artifacts.md` § "Networking host requirements".
- captured-by: m80/docs/behaviors/network/noegress-config.md#loopback-only + m80/m80-network/tests/network/noegress_config.rs::guest_sees_loopback_only

### Leaf: Touch no iptables rules when NoEgress is selected
- parent_var: $L2_10_1
- labels: $ACTIVE,network,noegress,iptables
- status: open
- behavior: The system makes no `iptables`, `sysctl`, or `ip` invocation under `NoEgress`, leaving host networking state unchanged.
- source: dossier `06-network-internals.md` § "Recommendation for v0.1"; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` lines 197-205.
- captured-by: m80/docs/behaviors/network/noegress-config.md#iptables-untouched + m80/m80-network/tests/network/noegress_config.rs::no_host_command_invoked

### Leaf: Record no_egress_reason in image manifest for operator audit
- parent_var: $L2_10_1
- labels: $ACTIVE,network,noegress,manifest
- status: open
- behavior: The system records a human-readable `no_egress_reason` field in the image manifest so operators can audit why a given VM was launched without egress.
- source: dossier `04-infra-and-artifacts.md` § Provenance manifest; predecessor `infra/firecracker/prepare-guestd-image.sh` lines 13, 311.
- captured-by: m80/docs/behaviors/network/noegress-config.md#no-egress-reason + m80/m80-network/tests/network/noegress_config.rs::no_egress_reason_recorded

## L2-10.2 Mode resolution (parent_var: $L2_10_2)

### Leaf: Collapse mode resolution to a single boolean for m80
- parent_var: $L2_10_2
- labels: $ACTIVE,network,resolve
- status: open
- behavior: The system resolves the VM network mode from a single `bool` (`request_network`) plus the workspace policy ceiling, replacing predecessor's `CapabilityClass::Network` plumbing.
- source: dossier `01-coupling-audit.md` § "agent-domain — `CapabilityClass`" + "Cost to remove"; predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `resolve_vm_network_mode` lines 155-169.
- captured-by: m80/docs/behaviors/network/mode-resolution.md#bool-input + m80/m80-network/tests/network/mode_resolution.rs::resolves_from_bool

### Leaf: Concentrate mode resolution behind a single seam function
- parent_var: $L2_10_2
- labels: $ACTIVE,network,resolve,seam
- status: open
- behavior: The system funnels every workspace-policy + network-request decision through one `resolve_vm_network_mode` function, so callers never branch on mode themselves.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs` `resolve_vm_network_mode` lines 155-169 (the only resolver entry point).
- captured-by: m80/docs/behaviors/network/mode-resolution.md#single-seam + m80/m80-network/tests/network/mode_resolution.rs::single_resolver_seam
