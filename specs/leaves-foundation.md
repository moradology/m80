# Leaf-bead spec: foundation L1s (preflight, boot, errors)

Source priority for every leaf below: m80 dossier first, then predecessor source
under `crates/sandbox/agent-sandbox-firecracker/src/`. Each leaf is a present-
tense behavior fact about the working predecessor implementation; the m80
acceptance criterion is "documented + tested in m80".

Skeleton variable names refer to entries in `specs/skeleton-ids.env`.

## L1-01 Host Preflight & Discovery

### L2-01.1 Binary discovery (parent_var: $L2_01_1)

### Leaf: Capture: firecracker binary resolution from env or default
- parent_var: $L2_01_1
- labels: $ACTIVE,preflight,configuration
- status: open
- behavior: The system resolves the Firecracker binary by reading the `FIRECRACKER_BIN` environment variable first and falls back to the managed install path under `/opt/firecracker/bin/firecracker` when the variable is unset.
- source: dossier `04-infra-and-artifacts.md` § "Firecracker and jailer binaries"; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:29` (`FIRECRACKER_BIN_ENV`) and lines 289-291 (`DiscoveryConfig::from_env`).
- captured-by: m80/docs/behaviors/preflight/binary-discovery.md#firecracker-binary + m80/m80-core/tests/preflight/binary_discovery.rs::firecracker_binary_resolves_from_env_then_default

### Leaf: Capture: jailer binary resolution from env or default
- parent_var: $L2_01_1
- labels: $ACTIVE,preflight,configuration
- status: open
- behavior: The system resolves the jailer binary by reading `JAILER_BIN` and falls back to `/opt/firecracker/bin/jailer` when the variable is unset.
- source: dossier `04-infra-and-artifacts.md` § "Firecracker and jailer binaries"; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:30` (`JAILER_BIN_ENV`) and lines 289-292 (`DiscoveryConfig::from_env`).
- captured-by: m80/docs/behaviors/preflight/binary-discovery.md#jailer-binary + m80/m80-core/tests/preflight/binary_discovery.rs::jailer_binary_resolves_from_env_then_default

### Leaf: Capture: managed artifact directory default
- parent_var: $L2_01_1
- labels: $ACTIVE,preflight,configuration
- status: open
- behavior: The system uses `/opt/firecracker/artifacts` as the default managed artifact directory unless overridden by the `FIRECRACKER_ARTIFACT_DIR` environment variable.
- source: dossier `04-infra-and-artifacts.md` § "Kernel"; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:37` (`DEFAULT_FIRECRACKER_ARTIFACT_DIR`).
- captured-by: m80/docs/behaviors/preflight/binary-discovery.md#managed-artifact-dir + m80/m80-core/tests/preflight/binary_discovery.rs::managed_artifact_dir_defaults

### Leaf: Capture: firecracker --version probe
- parent_var: $L2_01_1
- labels: $ACTIVE,preflight,configuration
- status: open
- behavior: The system invokes both binaries with `--version` during discovery and records the reported version string for later validation.
- source: dossier `04-infra-and-artifacts.md` § "Host preflight" item 5; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs` `QueryVersion`/`VersionCommandFailed` variants near lines 274-286.
- captured-by: m80/docs/behaviors/preflight/binary-discovery.md#version-probe + m80/m80-core/tests/preflight/binary_discovery.rs::version_command_runs_and_is_captured

### Leaf: Capture: expected firecracker version pin
- parent_var: $L2_01_1
- labels: $ACTIVE,preflight,configuration
- status: open
- behavior: The system fails closed when the binary's reported version differs from `FIRECRACKER_VERSION` whenever that environment variable is set.
- source: dossier `04-infra-and-artifacts.md` § "Firecracker and jailer binaries" (default `v1.15.1`); predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:33,301` (`FIRECRACKER_VERSION_ENV`, `expected_version`) and `errors.rs` `VersionMismatch` near line 288.
- captured-by: m80/docs/behaviors/preflight/binary-discovery.md#version-pin + m80/m80-core/tests/preflight/binary_discovery.rs::version_mismatch_fails_closed

### Leaf: Capture: missing firecracker binary fail-closed
- parent_var: $L2_01_1
- labels: $ACTIVE,preflight,errors
- status: open
- behavior: The system raises `FirecrackerBinaryNotFound` when neither the env override nor the managed path resolves to an executable Firecracker binary.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:152-153` (`FirecrackerBinaryNotFound`); dossier `04-infra-and-artifacts.md` § "Host preflight" item 3.
- captured-by: m80/docs/behaviors/preflight/binary-discovery.md#binary-not-found + m80/m80-core/tests/preflight/binary_discovery.rs::binary_not_found_returns_typed_error

### L2-01.2 KVM and OS gates (parent_var: $L2_01_2)

### Leaf: Capture: Linux-only host platform check
- parent_var: $L2_01_2
- labels: $ACTIVE,preflight,configuration
- status: open
- behavior: The system rejects hosts whose `uname -s` is not `Linux` and surfaces the actual platform string in the preflight error.
- source: dossier `04-infra-and-artifacts.md` § "Host preflight" item 1; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:1469` and `errors.rs:204` (`UnsupportedHostPlatform`).
- captured-by: m80/docs/behaviors/preflight/kvm-and-os-gates.md#linux-only + m80/m80-core/tests/preflight/kvm_and_os_gates.rs::non_linux_platform_fails_closed

### Leaf: Capture: /dev/kvm presence required
- parent_var: $L2_01_2
- labels: $ACTIVE,preflight,configuration
- status: open
- behavior: The system requires `/dev/kvm` to exist on the host and emits `KvmUnavailable` referencing the missing path otherwise.
- source: dossier `04-infra-and-artifacts.md` § "Host preflight" item 2; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:58` (`KVM_DEVICE_PATH`) and `errors.rs:206-207` (`KvmUnavailable`).
- captured-by: m80/docs/behaviors/preflight/kvm-and-os-gates.md#kvm-presence + m80/m80-core/tests/preflight/kvm_and_os_gates.rs::missing_kvm_node_fails_closed

### Leaf: Capture: /dev/kvm writable for current credentials
- parent_var: $L2_01_2
- labels: $ACTIVE,preflight,configuration
- status: open
- behavior: The system verifies the calling process can open `/dev/kvm` for read/write and emits `KvmNotWritable` when the device exists but cannot be written.
- source: dossier `04-infra-and-artifacts.md` § "Host preflight" item 2; predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:209-210` (`KvmNotWritable`).
- captured-by: m80/docs/behaviors/preflight/kvm-and-os-gates.md#kvm-writable + m80/m80-core/tests/preflight/kvm_and_os_gates.rs::kvm_not_writable_fails_closed

### Leaf: Capture: effective root or passwordless sudo gate
- parent_var: $L2_01_2
- labels: $ACTIVE,preflight,jailer
- status: open
- behavior: The system verifies the calling process is effectively root or has passwordless sudo before launching jailer-aware lifecycle paths.
- source: dossier `06-network-internals.md` § "Privilege model" lines 161-180; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:840-856` (`is_effective_root`, `verify_jailer_launch_privilege`).
- captured-by: m80/docs/behaviors/preflight/kvm-and-os-gates.md#privilege-gate + m80/m80-core/tests/preflight/kvm_and_os_gates.rs::privilege_gate_requires_root_or_passwordless_sudo

### Leaf: Capture: one-time host preflight at startup
- parent_var: $L2_01_2
- labels: $ACTIVE,preflight,lifecycle
- status: open
- behavior: The system performs the host platform/KVM/privilege gates as a one-shot preflight pass before each VM start rather than per-request.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:566-595` (HostPreflight diagnostic phase wraps `verify_start_vm_host_preflight`).
- captured-by: m80/docs/behaviors/preflight/kvm-and-os-gates.md#one-time-preflight + m80/m80-core/tests/preflight/kvm_and_os_gates.rs::host_preflight_runs_before_boot

### L2-01.3 Artifact and manifest preflight (parent_var: $L2_01_3)

### Leaf: Capture: kernel artifact auto-discovery under managed dir
- parent_var: $L2_01_3
- labels: $ACTIVE,preflight,image-build
- status: open
- behavior: The system auto-discovers the kernel image by listing `vmlinux-*` entries under the managed artifact directory and selecting the highest-versioned absolute path.
- source: dossier `04-infra-and-artifacts.md` § "Host preflight" item 6; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs` `ManagedArtifactNotFound` near line 197.
- captured-by: m80/docs/behaviors/preflight/artifact-and-manifest.md#kernel-auto-discovery + m80/m80-core/tests/preflight/artifact_and_manifest.rs::kernel_auto_discovery_picks_latest

### Leaf: Capture: rootfs path must be absolute
- parent_var: $L2_01_3
- labels: $ACTIVE,preflight,image-build
- status: open
- behavior: The system rejects rootfs paths that are not absolute and emits `NonAbsolutePath` carrying the offending kind and path.
- source: dossier `04-infra-and-artifacts.md` § "Host preflight" item 7; predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:191-192` (`NonAbsolutePath`).
- captured-by: m80/docs/behaviors/preflight/artifact-and-manifest.md#rootfs-absolute + m80/m80-core/tests/preflight/artifact_and_manifest.rs::rootfs_must_be_absolute

### Leaf: Capture: manifest schema_version 1 required next to rootfs
- parent_var: $L2_01_3
- labels: $ACTIVE,preflight,image-build
- status: open
- behavior: The system loads `<rootfs-path>.manifest.json` adjacent to the rootfs and validates that `schema_version == 1` before accepting the image.
- source: dossier `04-infra-and-artifacts.md` § "Provenance manifest" (schema version 1) and § "Host preflight" item 8; predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:247-265` (`ArtifactManifestNotFound`, `InvalidArtifactManifest`).
- captured-by: m80/docs/behaviors/preflight/artifact-and-manifest.md#manifest-schema + m80/m80-core/tests/preflight/artifact_and_manifest.rs::manifest_schema_version_one_required

### Leaf: Capture: manifest sha256 recomputation on load
- parent_var: $L2_01_3
- labels: $ACTIVE,preflight,image-build
- status: open
- behavior: The system recomputes sha256 over kernel, source rootfs, output rootfs, and guest daemon binary and refuses to boot when any digest disagrees with the manifest.
- source: dossier `04-infra-and-artifacts.md` § "Provenance manifest" fields list; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:1913-1934` (`expected_firecracker_version` block validates manifest fields) and `errors.rs:267-272` (`HashFile`).
- captured-by: m80/docs/behaviors/preflight/artifact-and-manifest.md#sha256-recompute + m80/m80-core/tests/preflight/artifact_and_manifest.rs::manifest_sha256_mismatch_fails_closed

### Leaf: Capture: run-root creatable and writable
- parent_var: $L2_01_3
- labels: $ACTIVE,preflight,storage
- status: open
- behavior: The system requires `FIRECRACKER_RUN_ROOT` (or its default) to be an absolute path that is creatable and writable, and emits `CreateRunDir` on failure.
- source: dossier `04-infra-and-artifacts.md` § "Host preflight" item 9; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:1759-1764` (`run_privileged_host_command("mkdir", ...)`, `chmod`) and `errors.rs:295-300` (`CreateRunDir`).
- captured-by: m80/docs/behaviors/preflight/artifact-and-manifest.md#run-root-writable + m80/m80-core/tests/preflight/artifact_and_manifest.rs::run_root_creatable_and_writable

### Leaf: Capture: storage helper binaries on PATH
- parent_var: $L2_01_3
- labels: $ACTIVE,preflight,storage
- status: open
- behavior: The system requires `mkfs.ext4`, `debugfs`, and `e2fsck` to be discoverable on PATH and emits `HelperBinaryNotFound` listing the missing program.
- source: dossier `04-infra-and-artifacts.md` § "Host preflight" item 10; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:63` (`REQUIRED_STORAGE_HELPERS`) and `errors.rs:200-201` (`HelperBinaryNotFound`).
- captured-by: m80/docs/behaviors/preflight/artifact-and-manifest.md#storage-helpers + m80/m80-core/tests/preflight/artifact_and_manifest.rs::storage_helpers_required_on_path

### Leaf: Capture: insufficient run-root capacity rejection
- parent_var: $L2_01_3
- labels: $ACTIVE,preflight,storage
- status: open
- behavior: The system queries available bytes under the run-root and rejects boots whose required bytes exceed available capacity via `InsufficientRunRootCapacity`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:231-245` (`QueryRunRootCapacity`, `InsufficientRunRootCapacity`); dossier `04-infra-and-artifacts.md` § "Host preflight" item 9.
- captured-by: m80/docs/behaviors/preflight/artifact-and-manifest.md#run-root-capacity + m80/m80-core/tests/preflight/artifact_and_manifest.rs::insufficient_run_root_capacity_fails_closed

### L2-01.4 Privileged-command shim (parent_var: $L2_01_4)

### Leaf: Capture: privileged-command shim entry point
- parent_var: $L2_01_4
- labels: $ACTIVE,preflight,jailer
- status: open
- behavior: The system routes every privileged host operation through `run_privileged_host_command()` rather than spawning sudo or root commands ad hoc throughout the codebase.
- source: dossier `06-network-internals.md` § "Privilege model" lines 161-180; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:860` (`run_privileged_host_command`).
- captured-by: m80/docs/behaviors/preflight/privileged-shim.md#single-entry + m80/m80-core/tests/preflight/privileged_shim.rs::all_privileged_calls_route_through_shim

### Leaf: Capture: shim direct vs sudo -n routing
- parent_var: $L2_01_4
- labels: $ACTIVE,preflight,jailer
- status: open
- behavior: The system invokes the privileged program directly when the process is effectively root and prefixes `sudo -n` only when the process is non-root and passwordless sudo is available.
- source: dossier `06-network-internals.md` § "Privilege model" lines 167-176; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:840-879` (`needs_sudo_passthrough`, `run_privileged_host_command`).
- captured-by: m80/docs/behaviors/preflight/privileged-shim.md#sudo-routing + m80/m80-core/tests/preflight/privileged_shim.rs::shim_uses_direct_when_root_and_sudo_when_not

### Leaf: Capture: shim non-interactive contract
- parent_var: $L2_01_4
- labels: $ACTIVE,preflight,jailer
- status: open
- behavior: The system always passes `-n` to sudo so privileged commands never prompt for a password and fail immediately when credentials are not cached.
- source: dossier `06-network-internals.md` § "Privilege model" lines 170-176; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:860-879` (`Command::new("sudo").arg("-n")`).
- captured-by: m80/docs/behaviors/preflight/privileged-shim.md#non-interactive + m80/m80-core/tests/preflight/privileged_shim.rs::shim_never_prompts_for_password

### Leaf: Capture: privileged-shim allowed program set
- parent_var: $L2_01_4
- labels: $ACTIVE,preflight,network
- status: open
- behavior: The system uses the privileged shim only for `ip`, `iptables`, `sysctl`, `mkdir`, `chmod`, `rm`, and `kill` host helpers required for run-root and network setup.
- source: dossier `06-network-internals.md` § "Privilege model" lines 182-186; predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:1584-1638,1759-1764,2397` (privileged calls invoke the listed programs).
- captured-by: m80/docs/behaviors/preflight/privileged-shim.md#allowed-programs + m80/m80-core/tests/preflight/privileged_shim.rs::shim_program_set_is_constrained

## L1-02 VM Boot Lifecycle

### L2-02.1 Run-directory layout (parent_var: $L2_02_1)

### Leaf: Capture: per-VM run directory keyed by vm_id
- parent_var: $L2_02_1
- labels: $ACTIVE,lifecycle,storage
- status: open
- behavior: The system creates the per-VM run directory at `<run_root>/<vm_id>/` and stores all VM-local artifacts beneath it.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:367-425` (`VmPaths::new`, `run_root.join(vm_id)`); dossier `07-modules-essential-vs-hygiene.md` § "lifecycle.rs".
- captured-by: m80/docs/behaviors/lifecycle/run-dir-layout.md#per-vm-dir + m80/m80-core/tests/lifecycle/run_dir_layout.rs::per_vm_dir_under_run_root

### Leaf: Capture: firecracker UDS api socket path
- parent_var: $L2_02_1
- labels: $ACTIVE,lifecycle,wire-protocol
- status: open
- behavior: The system places the Firecracker REST API Unix socket at `<run_dir>/firecracker.sock` and uses that path for every API request.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:409` (`api_socket: run_dir.join("firecracker.sock")`).
- captured-by: m80/docs/behaviors/lifecycle/run-dir-layout.md#api-socket + m80/m80-core/tests/lifecycle/run_dir_layout.rs::api_socket_path_is_firecracker_sock

### Leaf: Capture: vsock host-side socket path
- parent_var: $L2_02_1
- labels: $ACTIVE,lifecycle,vsock
- status: open
- behavior: The system places the vsock host-side bridge socket at `<run_dir>/vsock.sock` for the guest control channel.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:410` (`vsock_socket: run_dir.join("vsock.sock")`).
- captured-by: m80/docs/behaviors/lifecycle/run-dir-layout.md#vsock-socket + m80/m80-core/tests/lifecycle/run_dir_layout.rs::vsock_socket_path_is_vsock_sock

### Leaf: Capture: co-located rootfs clone and scratch image
- parent_var: $L2_02_1
- labels: $ACTIVE,lifecycle,storage
- status: open
- behavior: The system places the per-VM runtime rootfs clone at `<run_dir>/rootfs.ext4` and the workspace scratch image at `<run_dir>/workspace.ext4` adjacent to the sockets.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:413-414` (`runtime_rootfs`, `workspace_scratch`).
- captured-by: m80/docs/behaviors/lifecycle/run-dir-layout.md#image-paths + m80/m80-core/tests/lifecycle/run_dir_layout.rs::rootfs_and_scratch_paths_are_colocated

### Leaf: Capture: run directory reaped on delete
- parent_var: $L2_02_1
- labels: $ACTIVE,lifecycle,cleanup
- status: open
- behavior: The system removes the entire `<run_dir>` after a successful clean stop unless `preserve_run_dir` is set for diagnostic preservation.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:1368-1381` (`remove_run_dir_after_cleanup`); dossier `07-modules-essential-vs-hygiene.md` § "lifecycle.rs".
- captured-by: m80/docs/behaviors/lifecycle/run-dir-layout.md#reaped-on-delete + m80/m80-core/tests/lifecycle/run_dir_layout.rs::run_dir_removed_after_clean_stop

### L2-02.2 Pre-boot wiring (parent_var: $L2_02_2)

### Leaf: Capture: machine config PUT before boot
- parent_var: $L2_02_2
- labels: $ACTIVE,lifecycle,wire-protocol
- status: open
- behavior: The system PUTs `MachineConfig` (vCPU count, memory size, smt) over the Firecracker UDS API as the first wire step of every boot.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/boot.rs:374` (`client.put_machine_config`); `crates/sandbox/agent-sandbox-firecracker/src/client.rs:134` (`put_machine_config`).
- captured-by: m80/docs/behaviors/lifecycle/preboot-wiring.md#machine-config + m80/m80-core/tests/lifecycle/preboot_wiring.rs::machine_config_put_before_boot

### Leaf: Capture: boot source PUT with kernel and cmdline
- parent_var: $L2_02_2
- labels: $ACTIVE,lifecycle,wire-protocol
- status: open
- behavior: The system PUTs `BootSourceConfig` carrying the kernel image path and boot args before issuing `InstanceStart`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/boot.rs:115-122` (`BootSourceConfig` build) and `client.rs:138` (`put_boot_source`).
- captured-by: m80/docs/behaviors/lifecycle/preboot-wiring.md#boot-source + m80/m80-core/tests/lifecycle/preboot_wiring.rs::boot_source_put_before_instance_start

### Leaf: Capture: root drive PUT for runtime rootfs
- parent_var: $L2_02_2
- labels: $ACTIVE,lifecycle,storage
- status: open
- behavior: The system PUTs the runtime rootfs as `DriveConfig { is_root_device: true }` referencing the per-VM `rootfs.ext4` clone.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/boot.rs:39-77` (`MinimalBootRequest` shape) and `client.rs:142` (`put_drive`).
- captured-by: m80/docs/behaviors/lifecycle/preboot-wiring.md#root-drive + m80/m80-core/tests/lifecycle/preboot_wiring.rs::root_drive_put_with_is_root_device

### Leaf: Capture: scratch workspace drive PUT
- parent_var: $L2_02_2
- labels: $ACTIVE,lifecycle,storage
- status: open
- behavior: The system PUTs the scratch workspace as a non-root, read-write `DriveConfig` with `drive_id="workspace"` referencing `<run_dir>/workspace.ext4`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:665-670` (`boot_request.data_drives.push(DriveConfig { drive_id: "workspace".into(), ... })`).
- captured-by: m80/docs/behaviors/lifecycle/preboot-wiring.md#scratch-drive + m80/m80-core/tests/lifecycle/preboot_wiring.rs::scratch_drive_put_with_workspace_id

### Leaf: Capture: vsock device PUT with guest CID
- parent_var: $L2_02_2
- labels: $ACTIVE,lifecycle,vsock
- status: open
- behavior: The system PUTs a `VsockDeviceConfig` carrying the per-VM guest CID and the host UDS path before `InstanceStart`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:658-661` (`VsockDeviceConfig::guestd_for_vm`) and `client.rs:147` (`put_vsock`).
- captured-by: m80/docs/behaviors/lifecycle/preboot-wiring.md#vsock-device + m80/m80-core/tests/lifecycle/preboot_wiring.rs::vsock_device_put_before_start

### Leaf: Capture: boot identity recorded on success
- parent_var: $L2_02_2
- labels: $ACTIVE,lifecycle,image-build
- status: open
- behavior: The system writes a verified boot-identity manifest tying kernel, rootfs, manifest hash, guest port, ready marker, and boot target to the run directory after successful boot wiring.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:613,719` (`record_create_start_success(... &vm_paths.verified_boot_identity)`); dossier `07-modules-essential-vs-hygiene.md` § "boot.rs".
- captured-by: m80/docs/behaviors/lifecycle/preboot-wiring.md#boot-identity + m80/m80-core/tests/lifecycle/preboot_wiring.rs::boot_identity_recorded_on_success

### L2-02.3 UDS REST API client (parent_var: $L2_02_3)

### Leaf: Capture: synchronous HTTP-over-UDS client
- parent_var: $L2_02_3
- labels: $ACTIVE,wire-protocol,lifecycle
- status: open
- behavior: The system performs Firecracker REST calls synchronously over a `UnixStream` connection to the API socket without an async runtime.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/client.rs:2,291` (`use std::os::unix::net::UnixStream`, `UnixStream::connect`).
- captured-by: m80/docs/behaviors/lifecycle/uds-rest-client.md#sync-http-over-uds + m80/m80-core/tests/lifecycle/uds_rest_client.rs::client_uses_blocking_unix_stream

### Leaf: Capture: typed request configs
- parent_var: $L2_02_3
- labels: $ACTIVE,wire-protocol,configuration
- status: open
- behavior: The system carries `MachineConfig`, `BootSourceConfig`, `DriveConfig`, `NetworkInterfaceConfig`, and `VsockDeviceConfig` as typed Rust structs that serialize to the Firecracker REST schema.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/client.rs:134-159` (`put_machine_config`, `put_boot_source`, `put_drive`, `put_vsock`, `put_network_interface`).
- captured-by: m80/docs/behaviors/lifecycle/uds-rest-client.md#typed-configs + m80/m80-core/tests/lifecycle/uds_rest_client.rs::request_configs_serialize_to_schema

### Leaf: Capture: InstanceAction InstanceStart
- parent_var: $L2_02_3
- labels: $ACTIVE,wire-protocol,lifecycle
- status: open
- behavior: The system issues the `InstanceAction { action_type: InstanceStart }` POST to begin VM execution after pre-boot wiring is complete.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/client.rs:91-99,163-164` (`InstanceActionType::InstanceStart`).
- captured-by: m80/docs/behaviors/lifecycle/uds-rest-client.md#instance-start + m80/m80-core/tests/lifecycle/uds_rest_client.rs::instance_start_action_posts_after_wiring

### Leaf: Capture: InstanceAction SendCtrlAltDel on x86_64
- parent_var: $L2_02_3
- labels: $ACTIVE,wire-protocol,lifecycle
- status: open
- behavior: The system issues `InstanceAction { action_type: SendCtrlAltDel }` to request graceful guest shutdown only when the host architecture is x86_64.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/client.rs:99-100,173-174` (`InstanceActionType::SendCtrlAltDel`); `lifecycle.rs:1286,1297-1305` (arch-aware dispatch).
- captured-by: m80/docs/behaviors/lifecycle/uds-rest-client.md#send-ctrl-alt-del + m80/m80-core/tests/lifecycle/uds_rest_client.rs::send_ctrl_alt_del_only_on_x86_64

### Leaf: Capture: typed errors for API faults
- parent_var: $L2_02_3
- labels: $ACTIVE,wire-protocol,errors
- status: open
- behavior: The system maps API failures to typed variants `ApiConnect`, `ApiWrite`, `ApiRead`, `ApiSerialize`, `ApiDeserialize`, `InvalidHttpResponse`, and `ApiFault` carrying the offending endpoint and detail.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:858-901` (Api* variants).
- captured-by: m80/docs/behaviors/lifecycle/uds-rest-client.md#typed-errors + m80/m80-core/tests/lifecycle/uds_rest_client.rs::api_faults_produce_typed_variants

### Leaf: Capture: single-threaded blocking client
- parent_var: $L2_02_3
- labels: $ACTIVE,wire-protocol,concurrency
- status: open
- behavior: The system serializes Firecracker REST calls per VM through a blocking client that holds no internal connection pool.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/client.rs:2,291` (`UnixStream::connect` per-call) and dossier `07-modules-essential-vs-hygiene.md` § "client.rs".
- captured-by: m80/docs/behaviors/lifecycle/uds-rest-client.md#blocking-no-pool + m80/m80-core/tests/lifecycle/uds_rest_client.rs::client_is_blocking_no_pool

### L2-02.4 Start sequence and ready detection (parent_var: $L2_02_4)

### Leaf: Capture: serial console ready-marker probe
- parent_var: $L2_02_4
- labels: $ACTIVE,lifecycle,vsock
- status: open
- behavior: The system tails the per-VM console log for the `GUESTD_READY` marker before declaring the VM ready, treating the marker as the boot-completion signal.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:697` (`wait_for_console_marker(config.ready_marker, config.ready_timeout)`); dossier `07-modules-essential-vs-hygiene.md` § "vsock.rs".
- captured-by: m80/docs/behaviors/lifecycle/start-and-ready.md#console-marker + m80/m80-core/tests/lifecycle/start_and_ready.rs::ready_requires_console_marker

### Leaf: Capture: bounded ready timeout fails closed
- parent_var: $L2_02_4
- labels: $ACTIVE,lifecycle,errors
- status: open
- behavior: The system bounds the ready-marker wait by `ready_timeout` and surfaces `ConsoleMarkerTimeout` carrying the path, marker, and timeout when no marker arrives in time.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:937-942` (`ConsoleMarkerTimeout`); `lifecycle.rs:697`.
- captured-by: m80/docs/behaviors/lifecycle/start-and-ready.md#ready-timeout + m80/m80-core/tests/lifecycle/start_and_ready.rs::ready_timeout_fails_closed

### Leaf: Capture: post-marker vsock probe
- parent_var: $L2_02_4
- labels: $ACTIVE,lifecycle,vsock
- status: open
- behavior: The system follows the console-marker confirmation with a vsock probe over the guest control bridge before treating the VM as ready.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:698-704` (`VsockGuestChannel::guestd_default(...).probe()`).
- captured-by: m80/docs/behaviors/lifecycle/start-and-ready.md#vsock-probe + m80/m80-core/tests/lifecycle/start_and_ready.rs::ready_requires_vsock_probe_success

### Leaf: Capture: vsock guest port 9001
- parent_var: $L2_02_4
- labels: $ACTIVE,lifecycle,vsock
- status: open
- behavior: The system dials the guest control listener on the fixed vsock port `DEFAULT_GUESTD_VSOCK_PORT` (9001) once the console marker has fired.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:42` (`DEFAULT_GUESTD_VSOCK_PORT`); dossier `04-infra-and-artifacts.md` (`guest_port: 9001`).
- captured-by: m80/docs/behaviors/lifecycle/start-and-ready.md#guest-port + m80/m80-core/tests/lifecycle/start_and_ready.rs::guest_vsock_port_is_9001

### Leaf: Capture: full boot/stop per call
- parent_var: $L2_02_4
- labels: $ACTIVE,lifecycle,warm-pool
- status: open
- behavior: The system performs a fresh cold boot and clean stop for every lifecycle invocation in v0.1, with no warm-pool reuse path wired into the backend.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:531-722` (`StartedVm::create_and_start` is one-shot); dossier `07-modules-essential-vs-hygiene.md` § "blank_pool.rs".
- captured-by: m80/docs/behaviors/lifecycle/start-and-ready.md#cold-boot-only + m80/m80-core/tests/lifecycle/start_and_ready.rs::every_call_is_cold_boot

### L2-02.5 Graceful stop (parent_var: $L2_02_5)

### Leaf: Capture: x86_64 graceful via SendCtrlAltDel
- parent_var: $L2_02_5
- labels: $ACTIVE,lifecycle,cleanup
- status: open
- behavior: The system attempts a graceful stop on x86_64 by issuing `SendCtrlAltDel` and waiting up to the configured graceful timeout for the guest to power down on its own.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:970-981,1286,1297-1305` (`StopStrategy::GracefulThenForce` for x86_64; `client.send_ctrl_alt_del()`).
- captured-by: m80/docs/behaviors/lifecycle/graceful-stop.md#x86-graceful + m80/m80-core/tests/lifecycle/graceful_stop.rs::x86_64_uses_graceful_then_force

### Leaf: Capture: aarch64 forced stop
- parent_var: $L2_02_5
- labels: $ACTIVE,lifecycle,cleanup
- status: open
- behavior: The system skips the graceful path on non-x86_64 architectures and goes directly to forced termination because Firecracker does not support the keyboard-driven graceful path there.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:1297-1305` (`StopStrategy::ForceOnlyUnsupportedGraceful`).
- captured-by: m80/docs/behaviors/lifecycle/graceful-stop.md#non-x86-forced + m80/m80-core/tests/lifecycle/graceful_stop.rs::non_x86_uses_force_only

### Leaf: Capture: SIGKILL escalation after graceful timeout
- parent_var: $L2_02_5
- labels: $ACTIVE,lifecycle,cleanup
- status: open
- behavior: The system escalates to SIGKILL on the Firecracker process after the graceful timeout elapses without a clean exit, recording the failure as a forced-kill fallback.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:1308-1338` (`force_stop_and_cleanup`); `errors.rs:786-794` (`Terminate`, `WaitForExitTimeout`).
- captured-by: m80/docs/behaviors/lifecycle/graceful-stop.md#sigkill-escalation + m80/m80-core/tests/lifecycle/graceful_stop.rs::sigkill_after_graceful_timeout

### Leaf: Capture: idempotent re-invocation of stop
- parent_var: $L2_02_5
- labels: $ACTIVE,lifecycle,cleanup
- status: open
- behavior: The system tolerates repeated stop calls on an already-stopped VM and converges to a clean teardown without reporting a failure.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:1341-1366` (`cleanup_local_vm_artifacts` allows repeated cleanup); dossier `07-modules-essential-vs-hygiene.md` § "lifecycle.rs".
- captured-by: m80/docs/behaviors/lifecycle/graceful-stop.md#idempotent-stop + m80/m80-core/tests/lifecycle/graceful_stop.rs::stop_is_idempotent

### L2-02.6 Delete and recovery (parent_var: $L2_02_6)

### Leaf: Capture: delete tears down sockets, taps, state, run-dir
- parent_var: $L2_02_6
- labels: $ACTIVE,lifecycle,cleanup
- status: open
- behavior: The system tears down the API socket, vsock socket, network state, jailer artifacts, storage, and finally the run directory on delete in a deterministic order.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:1308-1381` (`force_stop_and_cleanup`, `remove_run_dir_after_cleanup`).
- captured-by: m80/docs/behaviors/lifecycle/delete-and-recovery.md#teardown-order + m80/m80-core/tests/lifecycle/delete_and_recovery.rs::delete_tears_down_in_order

### Leaf: Capture: stale run-root recovery on startup
- parent_var: $L2_02_6
- labels: $ACTIVE,lifecycle,cleanup
- status: open
- behavior: The system scavenges stale run-root entries via `recover_stale_run_root()` at the start of every lifecycle call and protects run-dirs whose lease cannot be acquired.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:540,753,1477` (`recover_stale_run_root`); dossier `07-modules-essential-vs-hygiene.md` § "lifecycle.rs".
- captured-by: m80/docs/behaviors/lifecycle/delete-and-recovery.md#recovery-on-startup + m80/m80-core/tests/lifecycle/delete_and_recovery.rs::stale_run_root_recovery_runs_at_startup

## L1-13 Errors & Failure Surfaces

### L2-13.1 Preflight error variants (parent_var: $L2_13_1)

### Leaf: Capture: FirecrackerBinaryNotFound preflight variant
- parent_var: $L2_13_1
- labels: $ACTIVE,errors,preflight
- status: open
- behavior: The system surfaces missing Firecracker binaries via the typed `FirecrackerBinaryNotFound` variant whose error text instructs the operator to set `FIRECRACKER_BIN` or install Firecracker on PATH.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:152-153`; dossier `07-modules-essential-vs-hygiene.md` § "errors.rs".
- captured-by: m80/docs/behaviors/errors/preflight-variants.md#binary-not-found + m80/m80-core/tests/errors/preflight_variants.rs::binary_not_found_carries_hint

### Leaf: Capture: KvmUnavailable preflight variant
- parent_var: $L2_13_1
- labels: $ACTIVE,errors,preflight
- status: open
- behavior: The system surfaces missing or unwritable `/dev/kvm` via `KvmUnavailable { path }` and `KvmNotWritable { path }` variants carrying the offending device path.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:206-210`.
- captured-by: m80/docs/behaviors/errors/preflight-variants.md#kvm-unavailable + m80/m80-core/tests/errors/preflight_variants.rs::kvm_unavailable_carries_path

### Leaf: Capture: UnsupportedHostPlatform preflight variant
- parent_var: $L2_13_1
- labels: $ACTIVE,errors,preflight
- status: open
- behavior: The system rejects non-Linux hosts via `UnsupportedHostPlatform { actual }` carrying the detected platform string for the operator.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:204` (`UnsupportedHostPlatform`).
- captured-by: m80/docs/behaviors/errors/preflight-variants.md#unsupported-host + m80/m80-core/tests/errors/preflight_variants.rs::unsupported_host_carries_actual

### Leaf: Capture: UnsupportedFirstLineVmSizing preflight variant
- parent_var: $L2_13_1
- labels: $ACTIVE,errors,preflight
- status: open
- behavior: The system rejects VM configs that deviate from the fixed 1 vCPU / 1024 MiB first-line sizing via `UnsupportedFirstLineVmSizing { vcpu_count, mem_size_mib }`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:226-229`.
- captured-by: m80/docs/behaviors/errors/preflight-variants.md#first-line-sizing + m80/m80-core/tests/errors/preflight_variants.rs::first_line_sizing_violation_typed

### Leaf: Capture: PrivilegedJailerLaunchUnavailable variant
- parent_var: $L2_13_1
- labels: $ACTIVE,errors,jailer
- status: open
- behavior: The system surfaces `PrivilegedJailerLaunchUnavailable` when the calling process is neither effective root nor able to invoke `sudo -n` and the jailer path is required.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:221-224`; `foundation.rs:847` (`verify_jailer_launch_privilege`).
- captured-by: m80/docs/behaviors/errors/preflight-variants.md#priv-jailer-unavail + m80/m80-core/tests/errors/preflight_variants.rs::priv_jailer_unavailable_typed

### L2-13.2 Lifecycle error variants (parent_var: $L2_13_2)

### Leaf: Capture: ApiSocketTimeout lifecycle variant
- parent_var: $L2_13_2
- labels: $ACTIVE,errors,lifecycle
- status: open
- behavior: The system raises `ApiSocketTimeout { path, timeout }` when the Firecracker process fails to expose its API socket within the configured timeout, signalling pre-boot failure.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:778-781`.
- captured-by: m80/docs/behaviors/errors/lifecycle-variants.md#api-socket-timeout + m80/m80-core/tests/errors/lifecycle_variants.rs::api_socket_timeout_typed

### Leaf: Capture: ConsoleMarkerTimeout lifecycle variant
- parent_var: $L2_13_2
- labels: $ACTIVE,errors,lifecycle
- status: open
- behavior: The system raises `ConsoleMarkerTimeout { path, marker, timeout }` when the console log fails to emit the configured ready marker within the ready-detection budget.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:937-942`.
- captured-by: m80/docs/behaviors/errors/lifecycle-variants.md#console-marker-timeout + m80/m80-core/tests/errors/lifecycle_variants.rs::console_marker_timeout_typed

### Leaf: Capture: typed lifecycle failure kinds
- parent_var: $L2_13_2
- labels: $ACTIVE,errors,lifecycle
- status: open
- behavior: The system classifies lifecycle failures into the bounded set `GuestdNotReady`, `BrokenVsock`, `StuckVm`, `GracefulStopTimeout`, `ForcedKillFallback`, `CleanupFailure`, and `WritebackSkippedAfterUncleanStop` via `LifecycleFailureKind`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:12-23` (`LifecycleFailureKind`); dossier `07-modules-essential-vs-hygiene.md` § "errors.rs".
- captured-by: m80/docs/behaviors/errors/lifecycle-variants.md#failure-kinds + m80/m80-core/tests/errors/lifecycle_variants.rs::lifecycle_failure_kinds_bounded

### Leaf: Capture: UnsupportedSnapshotLaunchMode for v0.1
- parent_var: $L2_13_2
- labels: $ACTIVE,errors,snapshot
- status: open
- behavior: The system raises `UnsupportedSnapshotLaunchMode { mode }` whenever a caller requests snapshot creation in v0.1, since the execution lane is deferred to v0.2.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:167-170`.
- captured-by: m80/docs/behaviors/errors/lifecycle-variants.md#unsupported-snapshot + m80/m80-core/tests/errors/lifecycle_variants.rs::unsupported_snapshot_launch_typed

### L2-13.3 Error to CLI mapping (parent_var: $L2_13_3)

### Leaf: Capture: error class to CLI exit-code map
- parent_var: $L2_13_3
- labels: $ACTIVE,errors,observability
- status: open
- behavior: The system maps preflight errors, lifecycle errors, and unknown errors to distinct non-zero exit codes so callers can branch on outcome class without parsing stderr.
- source: dossier `07-modules-essential-vs-hygiene.md` § "errors.rs"; predecessor `crates/sandbox/agent-sandbox-firecracker/src/errors.rs:149-948` (typed variants drive CLI surface).
- captured-by: m80/docs/behaviors/errors/cli-mapping.md#exit-codes + m80/m80-cli/tests/errors/cli_mapping.rs::error_classes_distinct_exit_codes

### Leaf: Capture: machine-readable JSON error envelope
- parent_var: $L2_13_3
- labels: $ACTIVE,errors,observability
- status: open
- behavior: The system emits a stable JSON envelope on stderr when callers pass `--json` so error class, variant name, and detail can be parsed without natural-language scraping.
- source: dossier `07-modules-essential-vs-hygiene.md` § "errors.rs"; m80 plan `/home/nathan/.claude/plans/ok-so-your-goal-eager-sphinx.md` § "L2-13.3 Error → CLI mapping".
- captured-by: m80/docs/behaviors/errors/cli-mapping.md#json-envelope + m80/m80-cli/tests/errors/cli_mapping.rs::json_envelope_is_stable

### Leaf: Capture: non-error stderr is informational
- parent_var: $L2_13_3
- labels: $ACTIVE,errors,observability
- status: open
- behavior: The system reserves stderr for human-readable progress and log lines while errors flow only through the typed envelope or exit code, so successful runs may emit stderr without being treated as failures.
- source: dossier `07-modules-essential-vs-hygiene.md` § "errors.rs"; m80 plan `/home/nathan/.claude/plans/ok-so-your-goal-eager-sphinx.md` § "L2-13.3 Error → CLI mapping".
- captured-by: m80/docs/behaviors/errors/cli-mapping.md#stderr-informational + m80/m80-cli/tests/errors/cli_mapping.rs::stderr_does_not_imply_failure
