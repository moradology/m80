# `m80-preflight`

Pre-launch host validation: KVM, CPU virtualization flags, kernel modules,
cgroup mode, binaries, manifest, storage helpers, run-root capacity. Emits a
tabular report and a
machine-readable `Discovery` value.

## Reason for being

Half the bug reports against any VM-orchestration tool are "it didn't
work" without a clear "because the host is missing X". A tight,
fail-closed preflight crate is the cheapest way to convert those into
"because /dev/kvm is not writable by this user — try `chmod g+rw /dev/kvm`."

Centralizing preflight also collects every host assumption in one place,
which is the right place for a security review to start.

## Black-box contract

- `run() -> Result<Discovery, PreflightError>` performs the full check
  list and returns a `Discovery` carrying every resolved path, version,
  and capability the rest of the system needs. Any missing or invalid
  capability returns a typed error; there's no partial-success mode.
- `verify_host_substrate(host_feature_config) -> Result<HostSubstrateDiscovery,
  PreflightError>` runs the non-mutating Linux/KVM/cgroup/jail-identity/
  privilege subset used before install finalization and before launch.
  `verify_host_substrate_fixture(host_feature_config, fixture)` runs the same
  classifier against `HostSubstrateFixture` without touching host `/dev/kvm`,
  cgroups, passwd/group databases, or real capabilities. Both return a
  proof-kind row so hostless fixtures cannot be mistaken for a real-KVM run
  smoke.
- The check list is fixed and ordered:
  1. **OS gate** — `Linux` from `uname -s`; macOS rejects.
  2. **Host kernel floor** — `uname -r` must parse as Linux kernel 6.1 or
     newer. Older or unparseable releases fail before any KVM or artifact work.
  3. **KVM** — `/dev/kvm` exists and is writable by this process.
  4. **KVM CPU extensions** — `/proc/cpuinfo` advertises at least one of
     `vmx` or `svm`, so launch failures from disabled hardware
     virtualization surface before Firecracker startup.
  5. **Kernel modules / devices** — `bridge` and `tap` loaded (read from
     `/proc/modules`), TUN available through either the `tun` module or
     `/dev/net/tun`, vhost-vsock available through either the `vhost_vsock`
     module or `/dev/vhost-vsock`, and `nf_conntrack` available for outbound
     NAT. `br_netfilter` must also be available and
     `/proc/sys/net/bridge/bridge-nf-call-iptables` must be `1`, so bridged
     TAP traffic traverses the TAP-scoped iptables rules. v0.1 does not
     attempt to load missing modules or change sysctls; the operator must do
     that before running preflight.
  5b. **Conntrack capacity** — reads
     `/proc/sys/net/netfilter/nf_conntrack_max` and fails if the host-global
     conntrack table is below `2 * M80_MAX_CONCURRENT_VMS * 1000` entries.
     `M80_MAX_CONCURRENT_VMS` defaults to `8`.
  6. **Transparent hugepages** — reads
     `/sys/kernel/mm/transparent_hugepage/enabled` and emits a non-blocking
     informational row. `[always]` is a clean row. `[madvise]`, `[never]`,
     unreadable, or unrecognized policy text still pass preflight but carry an
     advisory detail pointing operators at host tuning docs.
  7. **KVM halt polling** — reads `/sys/module/kvm/parameters/halt_poll_ns`
     and related KVM timer knobs, then emits a non-blocking informational row
     pointing operators at latency-priority versus density-priority guidance.
  8. **CPU governor** — reads
     `/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver` and
     `scaling_governor`, then emits a non-blocking row. `acpi-cpufreq` with a
     non-`performance` governor carries an advisory; `intel_pstate` and
     `amd_pstate` stay clean because their hardware-managed ramp behavior is
     different.
  9. **CPU microcode** — reads CPU0 microcode `version` and `processor_flags`
     sysfs rows when present and emits a non-blocking report row. Missing
     microcode sysfs data is preserved as `unavailable` detail rather than
     guessed.
  10. **CPU vulnerabilities** — reads selected files under
     `/sys/devices/system/cpu/vulnerabilities`. `mds` and `l1tf` reporting
     `Vulnerable` fail preflight unless
     `M80_SKIP_CHECK_VULNERABILITIES=1` is set. Other vulnerability files emit
     advisory report detail when the kernel reports `Vulnerable` or an
     unclassified status.
  11. **Cgroup mode** — when effective `cgroup_mode` is `unified-v2`,
     `m80-cgroup::Subtree::probe()` must confirm a unified cgroup v2 hierarchy
     before launch work begins. `cgroup_mode = "disabled"` skips this check.
  12. **Jailer identity** — effective `jail_uid` and `jail_gid` must resolve
      through the host passwd and group databases before launch. Preflight does
      not create identities or silently fall back to another UID/GID.
  13. **Privilege** — `geteuid() == 0` OR the effective Linux capability set
     contains every entry in `REQUIRED_CAPABILITIES`
     (`CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, `CAP_MKNOD`, `CAP_CHOWN`,
     `CAP_FOWNER`, `CAP_KILL`, `CAP_SETUID`, `CAP_SETGID`, `CAP_SETPCAP`).
     `CAP_SETPCAP` is required so `m80-jailer-harden` can prune the official
     jailer's bounding set and so `m80-firecracker` can drop `CAP_NET_ADMIN`
     from the backend thread after `m80-net-helper` starts. Probed via the
     `caps` crate against the process's effective set. Returns
     `PrivilegeStatus::Root` or `PrivilegeStatus::CapabilityBearing`.
  13b. **Host substrate proof** — records whether the substrate rows came
      from live preflight or a hostless fixture. Neither value is a real-KVM
      `m80 run -- echo hello` smoke by itself.
  14. **Firecracker binary** — discovered via env override or default. The
     path must be absolute, and `--version` must clear the documented CVE floor
     before any configured exact version pin is accepted. After artifact
     verification, the probed version must also match the guest manifest
     `expected_firecracker_version`.
  15. **Jailer binary** — absolute path, discovered via env override or
      default. `jailer --version` must parse as an official release and match
      the accepted Firecracker version exactly.
  16. **Jailer hardening wrapper** — `m80-jailer-harden`, discovered via
     `M80_JAILER_HARDEN_BIN` or `/opt/m80/bin/m80-jailer-harden`.
  16b. **Network helper** — `m80-net-helper`, discovered via
     `M80_NET_HELPER_BIN` or `/opt/m80/bin/m80-net-helper`.
  16c. **Host binary manifest** — reads
      `<artifact_dir>/host-binaries.manifest.json`, requires entries for
      `firecracker`, `jailer`, `m80`, `m80_jailer_harden`, and
      `m80_net_helper`, plus a `launch_material` entry for
      `firecracker_seccomp_filter`. It checks the configured paths for the
      runtime-selected binaries and seccomp filter, opens every recorded path
      with `O_NOFOLLOW`, hashes the opened file descriptor, rejects
      non-root-owned or group/world-writable binaries and launch material, and
      rejects empty launch-material files. It also compares recorded versions
      against live Firecracker/jailer discovery, m80 helper `--version`
      output, and the seccomp filter's owning Firecracker train.
  17. **Kernel artifact** — auto-discovered as the latest `vmlinux-*`
     under `<artifact_dir>`, or the env-overridden absolute path. When
     `M80_KERNEL_KIND=stock|stripped` is set, the discovered manifest's
     `kernel_kind` is overridden to match the selected kernel artifact.
  18. **Rootfs + manifest + build receipt** — manifest schema validates,
     including `rootfs_format`, and `m80-image-manifest::verify` recomputes every sha256.
     `<rootfs>.build-receipt.json` must point at the same manifest, its
     manifest sha256 must match the manifest bytes, and its artifact path/hash
     tuples must match the manifest.
     The rootfs itself is opened with `O_RDONLY | O_NOFOLLOW`, hashed from
     that descriptor, rewound, and kept in `Discovery` for launch to bind via
     procfs. This is the boot-artifact trust boundary for `m80-firecracker`;
     launch phase 3 does not re-open the original rootfs path.
     Group/world-writable rootfs files and artifact directories fail closed.
  19. **Run-root** — absolute, must already exist, >= 100 MiB free
     (no silent creation; caller must ensure the directory is present).
  20. **Run-root filesystem** — creates a short-lived probe file under the
     run-root and runs `cp --reflink=always` to report whether the filesystem
     supports metadata-only CoW clones. This advisory is non-blocking:
     unsupported reflinks mean callers should choose explicit byte-copy mode
     for that run-root, or let explicit auto mode select byte-copy.
  21. **Storage helpers** — `mkfs.ext4`, `cp`, `fallocate`, `debugfs`,
      `e2fsck` on PATH.
- A boot-scoped sentinel under `/run/m80-preflight-ok-<sha256>` caches only
  immutable-artifact outputs: `firecracker --version`, `jailer --version`, and
  `m80-image-manifest::verify`. Host binary sha256 verification still runs on
  every preflight invocation. The key includes the kernel boot id, configured
  Firecracker version pin, kernel kind override, and metadata for the
  Firecracker, Firecracker seccomp filter, jailer, hardening wrapper, network
  helper, kernel, rootfs, and manifest files.
  Corrupt or mismatched sentinels are ignored and rewritten after a successful
  full check. `M80_FORCE_PREFLIGHT=1` disables the cache for that invocation.
- Env keys are exact and case-sensitive. Preflight recognizes
  `M80_FIRECRACKER_BIN`, `M80_FIRECRACKER_VERSION`, `M80_JAILER_BIN`,
  `M80_JAILER_HARDEN_BIN`, `M80_NET_HELPER_BIN`, `M80_KERNEL_IMAGE`, `M80_ARTIFACT_DIR`,
  `M80_ROOTFS_IMAGE`, `M80_KERNEL_KIND`, `M80_RUN_ROOT`, `M80_JAIL_UID`,
  `M80_JAIL_GID`, `M80_CGROUP_MODE`, `M80_MAX_CONCURRENT_VMS`, and
  `M80_FORCE_PREFLIGHT`.
  `M80_SKIP_CHECK_VULNERABILITIES=1` is a documented
  escape hatch for the CPU vulnerability hard gate. The full schema is captured
  in `docs/behaviors/configuration/env-schema.md`.
- Each check produces a row in the `Discovery::report` field. The same
  data is rendered as a fixed-width table for human consumption via
  `Discovery::render_table()`.
- Errors carry "what to try next" hints. For example,
  `KvmUnavailable { path }` names the missing device, `KvmNotWritable { path }`
  includes the suggestion to add the user to the `kvm` group, and
  `FirecrackerVersionMismatch` includes both the expected and actual versions
  plus the source-of-truth file and policy doc to repair from.
- Firecracker CVE floor maintenance is documented in
  `docs/security/firecracker-cve-floor.md` and
  `docs/decisions/0004-firecracker-cve-floor-bump-process.md`.

## Public surface

- `run() -> Result<Discovery, PreflightError>`.
- `run_with_configs(binary_config: BinaryDiscoveryConfig, artifact_config: ArtifactPreflightConfig, host_feature_config: HostFeaturePreflightConfig) -> Result<Discovery, PreflightError>` — composable entry point that accepts pre-built config structs rather than reading env vars internally.
- `verify_host_substrate(host_feature_config: HostFeaturePreflightConfig)
  -> Result<HostSubstrateDiscovery, PreflightError>` — reusable non-mutating
  substrate verifier for install and preflight.
- `verify_host_substrate_fixture(host_feature_config: HostFeaturePreflightConfig,
  fixture: &HostSubstrateFixture) -> Result<HostSubstrateDiscovery,
  PreflightError>` — hostless fixture verifier for release/install CI.
- `BinaryDiscoveryConfig { firecracker_bin, firecracker_seccomp_filter,
  jailer_bin, jailer_harden_bin, net_helper_bin, expected_firecracker_version }` and
  `BinaryDiscoveryConfig::from_env()` for
  explicit `run_with_configs` callers. `expected_firecracker_version` is an
  optional pre-artifact pin; `run_with_configs` always rechecks the probed
  Firecracker version against the verified guest manifest. There is no
  `Default`; callers must use `from_env()` or construct the full effective
  config.
- `HostBinariesManifestConfig { firecracker_bin, firecracker_seccomp_filter,
  jailer_bin, jailer_harden_bin, net_helper_bin, m80_bin,
  expected_firecracker_version }`, `HostBinariesManifestConfig::from_env()`,
  `generate_host_binaries_manifest(&config)`, and
  `write_host_binaries_manifest(&config, path)` for install-time manifest
  generation from final host paths.
- `ArtifactPreflightConfig { kernel_image, artifact_dir, rootfs_image,
  kernel_kind, run_root, helper_search_path }` and
  `ArtifactPreflightConfig::from_env()` for explicit `run_with_configs`
  callers. There is no `Default`; callers must use `from_env()` or construct
  the full effective config.
- `HostFeaturePreflightConfig { cgroup_mode, jail_uid, jail_gid }` and
  `expected_concurrent_vms }` and
  `HostFeaturePreflightConfig::from_env() -> Result<Self, PreflightError>`,
  plus `CgroupPreflightMode { UnifiedV2, Disabled }`, for checks whose
  required host features depend on effective config.
- `HostSubstrateDiscovery { proof_kind, privilege, report }` and
  `HostSubstrateProofKind { LivePreflight, HostlessFixture }`.
- `HostSubstrateFixture { sysname, kernel_release, kvm, cgroup_v2_available,
  jail_user, jail_group, euid, effective_caps }` plus
  `HostSubstrateFixture::supported_root()` and
  `HostSubstrateFixtureKvm { Writable, Missing, NotWritable }`.
- `classify_privilege(euid, effective_caps) -> Result<PrivilegeStatus,
  PreflightError>` — pure classifier used by the live privilege probe and
  focused tests.
- `Discovery { firecracker_bin, firecracker_seccomp_filter, jailer_bin,
  jailer_harden_bin, net_helper_bin, kernel: PathBuf, rootfs: PathBuf, pinned_rootfs: PinnedRootfs,
  manifest: m80_image_manifest::Manifest, run_root: PathBuf,
  privilege: PrivilegeStatus, report: Vec<CheckRow> }`.
- `PinnedRootfs::from_file(path, file) -> PinnedRootfs`,
  `PinnedRootfs::path() -> &Path`, and
  `PinnedRootfs::proc_fd_path() -> PathBuf`.
- `Discovery::render_table()` → `String`.
- `CheckRow { label, passed, detail }`.
- `PrivilegeStatus { Root, CapabilityBearing }`.
- `FirecrackerTrainPolicy::from_expected_firecracker_version`, plus
  `expected_firecracker_version()`, `jailer_pairing_rule()`, `cve_floor()`,
  `source()`, `cve_floor_source()`, and `policy_doc()` — the shared source for
  Firecracker train, official jailer pairing, and CVE-floor metadata.
- `active_firecracker_cve_floors() -> &'static [FirecrackerCveFloor]` and
  `FirecrackerCveFloor { cve_id, expected }`.
- `FIRECRACKER_TRAIN_POLICY_SOURCE`, `FIRECRACKER_CVE_FLOOR_SOURCE`,
  `HOST_PREREQUISITE_POLICY_DOC`, and `JAILER_PAIRING_RULE`.
- `REQUIRED_CAPABILITIES: &[caps::Capability]` — the per-call cap list
  (`CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, `CAP_MKNOD`, `CAP_CHOWN`,
  `CAP_FOWNER`, `CAP_KILL`, `CAP_SETUID`, `CAP_SETGID`, `CAP_SETPCAP`).
- `ENV_FIRECRACKER_BIN`, `ENV_FIRECRACKER_SECCOMP_FILTER`,
  `ENV_FIRECRACKER_VERSION`, `DEFAULT_FIRECRACKER_BIN`,
  `DEFAULT_FIRECRACKER_SECCOMP_FILTER`, `ENV_KERNEL_IMAGE`,
  `ENV_KERNEL_KIND`, and `ENV_ROOTFS_IMAGE` — shared keys/default used by the
  CLI when it displays or overlays effective preflight inputs. Other env
  keys/default paths remain crate-private implementation details of
  `from_env()`.
- `PreflightError`: `UnsupportedHostPlatform { actual }`,
  `HostKernelUnsupported { actual, minimum }`,
  `KvmUnavailable { path }`, `KvmNotWritable { path }`,
  `KvmCpuExtensionMissing`, `InvalidCgroupMode { actual }`,
  `InvalidJailIdentity { field, value }`,
  `JailIdentityUnavailable { field, id }`,
  `CpuVulnerabilityDetected { id, detail }`,
  `CgroupV2Unavailable`, `VsockUnavailable`, `TunUnavailable`,
  `NfConntrackUnavailable`,
  `NfConntrackCapacityTooLow { actual, minimum, expected_concurrent_vms }`,
  `InvalidNfConntrackMax { actual }`,
  `InvalidExpectedConcurrentVms { actual }`,
  `KernelModulesMissing { missing: Vec<String> }`,
  `PrivilegeUnavailable { missing_caps: Vec<caps::Capability> }`,
  `FirecrackerBinaryNotFound`,
  `FirecrackerVersionMismatch { expected, actual, policy_source }`,
  `FirecrackerVersionCommandFailed { path, status }`,
  `FirecrackerVersionOutputMalformed { actual, policy_source }`,
  `FirecrackerCveFloorViolation { cve_id, actual, expected, policy_source }`,
  `FirecrackerSeccompFilterNotFound { path }`,
  `FirecrackerSeccompFilterEmpty { path }`,
  `CapabilityRead(caps::errors::CapsError)` (failed to read the process's
  effective capability set),
  `JailerBinaryNotFound`,
  `JailerVersionCommandFailed { path, status }`,
  `JailerVersionOutputMalformed { actual, policy_source }`,
  `JailerVersionMismatch { expected, actual, policy_source }`,
  `JailerHardenBinaryNotFound`,
  `HostBinaryManifest(m80_image_manifest::ManifestError)`,
  `HostBinaryMissing { name }`, `HostBinaryDuplicate { name }`,
  `HostBinaryPathMismatch { name, expected, actual }`,
  `BinaryHashMismatch { name, path, expected, actual }`,
  `HostBinaryPermission { name, path, reason }`,
  `HostLaunchMaterialMissing { name }`,
  `HostLaunchMaterialDuplicate { name }`,
  `HostLaunchMaterialPathMismatch { name, expected, actual }`,
  `HostLaunchMaterialHashMismatch { name, path, expected, actual }`,
  `HostLaunchMaterialPermission { name, path, reason }`,
  `NonAbsolutePath { kind, path }`,
  `KernelNotFound`, `RootfsNotFound`,
  `ArtifactDirectoryWritable { path, mode }`,
  `ArtifactFileWritable { path, mode }`,
  `Manifest(m80_image_manifest::ManifestError)`,
  `BuildReceipt(m80_image_manifest::ManifestError)`,
  `BuildReceiptPathMismatch { expected, actual }`,
  `BuildReceiptManifestMismatch { path, expected, actual }`,
  `BuildReceiptArtifactMissing { kind }`,
  `BuildReceiptArtifactDuplicate { kind }`,
  `BuildReceiptArtifactPathMismatch { kind, expected, actual }`,
  `BuildReceiptArtifactHashMismatch { kind, expected, actual }`,
  `RunRootUnavailable { reason: String }` (covers both missing-dir and
  insufficient-space), `StorageHelperMissing(String)`, `PathIo { path, source }`,
  and `SystemIo { operation, source }`.
- `PreflightError::hint() -> &'static str`.

## Non-goals

- **No remediation.** This crate reports; it does not chmod, chgrp,
  install packages, or download artifacts.
- **No image build.** That's `m80-image-build`. Preflight only verifies
  what's on disk.
- **No per-VM checks.** Capacity-per-VM, port availability, IP collision
  — those happen at boot time inside `m80-firecracker`, not here.
- **No daemon mode.** `run()` is a one-shot.

## Dependencies

- `m80-cgroup` — for cgroup v2 mode probing.
- `m80-image-manifest` — for manifest validation.
- `caps` — for reading the process's effective Linux capability set.
- `hex`, `serde`, `sha2`, `thiserror`, `tracing`, `nix`.

## Tests

- Happy path on a real KVM-capable host (CI behind a `kvm` feature flag).
- Each `PreflightError` variant has a dedicated negative test that
  arranges the host fixture to fail that check and asserts the typed
  error.
- Report determinism: same host state produces the same `report` rows
  in the same order.
- Hint coverage: every error variant carries a non-empty hint string.
- Host prerequisite fixtures: `tests/host_prerequisite_fixtures.rs` builds a
  full fake install root and covers success, wrong Firecracker train, stale
  byte-observable, missing jailer/seccomp, helper version-probe failure, and
  install-root override without touching real `/opt`, `/etc`, `/dev/kvm`,
  cgroups, or sudo. The final root-owned file/mode/hash rejection checks live
  in the host-binary verifier/core tests.
- Sentinel cache: corrupt sentinel rewrite, boot-id invalidation, and rootfs
  metadata invalidation.
- `tests/security/rootfs_fd_pinning.rs` — proc-fd handle continues to read the
  verified rootfs bytes after the original path is replaced.
- `crates/m80-preflight/src/artifacts.rs::tests::missing_build_receipt_returns_typed_io_error`.
- `crates/m80-preflight/src/artifacts.rs::tests::build_receipt_manifest_sha_mismatch_fails_preflight`.
- `crates/m80-preflight/src/artifacts.rs::tests::build_receipt_artifact_hash_must_match_manifest`.
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_hash_mismatch_fails_closed`.
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_rejects_path_mismatch`.
- `crates/m80-preflight/src/binary.rs::tests::host_binary_manifest_rejects_unsafe_permissions`.
