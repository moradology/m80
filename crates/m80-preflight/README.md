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
- The check list is fixed and ordered:
  1. **OS gate** — `Linux` from `uname -s`; macOS rejects.
  2. **KVM** — `/dev/kvm` exists and is writable by this process.
  3. **KVM CPU extensions** — `/proc/cpuinfo` advertises at least one of
     `vmx` or `svm`, so launch failures from disabled hardware
     virtualization surface before Firecracker startup.
  4. **Kernel modules / devices** — `bridge` and `tap` loaded (read from
     `/proc/modules`), TUN available through either the `tun` module or
     `/dev/net/tun`, vhost-vsock available through either the `vhost_vsock`
     module or `/dev/vhost-vsock`, and `nf_conntrack` available for outbound
     NAT. v0.1 does not attempt to load missing modules; the operator must
     `modprobe` them before running preflight.
  5. **Transparent hugepages** — reads
     `/sys/kernel/mm/transparent_hugepage/enabled` and emits a non-blocking
     informational row. `[always]` is a clean row. `[madvise]`, `[never]`,
     unreadable, or unrecognized policy text still pass preflight but carry an
     advisory detail pointing operators at host tuning docs.
  6. **KVM halt polling** — reads `/sys/module/kvm/parameters/halt_poll_ns`
     and related KVM timer knobs, then emits a non-blocking informational row
     pointing operators at latency-priority versus density-priority guidance.
  7. **CPU governor** — reads
     `/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver` and
     `scaling_governor`, then emits a non-blocking row. `acpi-cpufreq` with a
     non-`performance` governor carries an advisory; `intel_pstate` and
     `amd_pstate` stay clean because their hardware-managed ramp behavior is
     different.
  8. **CPU vulnerabilities** — reads selected files under
     `/sys/devices/system/cpu/vulnerabilities`. `mds` and `l1tf` reporting
     `Vulnerable` fail preflight unless
     `M80_SKIP_CHECK_VULNERABILITIES=1` is set. Other vulnerability files emit
     advisory report detail when the kernel reports `Vulnerable` or an
     unclassified status.
  9. **Cgroup mode** — when effective `cgroup_mode` is `unified-v2`,
     `m80-cgroup::Subtree::probe()` must confirm a unified cgroup v2 hierarchy
     before launch work begins. `cgroup_mode = "disabled"` skips this check.
  10. **Privilege** — `geteuid() == 0` OR the effective Linux capability set
     contains every entry in `REQUIRED_CAPABILITIES`
     (`CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, `CAP_MKNOD`, `CAP_CHOWN`,
     `CAP_FOWNER`, `CAP_KILL`). Probed via the `caps` crate against the
     process's effective set. Returns `PrivilegeStatus::Root` or
     `PrivilegeStatus::CapabilityBearing`.
  11. **Firecracker binary** — discovered via env override or default,
     `--version` must clear the documented CVE floor before any configured
     exact version pin is accepted.
  12. **Jailer binary** — same protocol.
  13. **Jailer hardening wrapper** — `m80-jailer-harden`, discovered via
     `M80_JAILER_HARDEN_BIN` or `/opt/m80/bin/m80-jailer-harden`.
  14. **Kernel artifact** — auto-discovered as the latest `vmlinux-*`
     under `<artifact_dir>`, or the env-overridden absolute path. When
     `M80_KERNEL_KIND=stock|stripped` is set, the discovered manifest's
     `kernel_kind` is overridden to match the selected kernel artifact.
  15. **Rootfs + manifest** — manifest schema validates, including
     `rootfs_format`, and `m80-image-manifest::verify` recomputes every sha256. This is the
     boot-artifact trust boundary for `m80-firecracker`; launch phase 3 does
     not rehash these artifacts again for every VM.
  16. **Run-root** — absolute, must already exist, >= 100 MiB free
     (no silent creation; caller must ensure the directory is present).
  17. **Run-root filesystem** — creates a short-lived probe file under the
     run-root and runs `cp --reflink=always` to report whether the filesystem
     supports metadata-only CoW clones. This advisory is non-blocking:
     unsupported reflinks mean launch falls back through `cp --reflink=auto`.
  18. **Storage helpers** — `mkfs.ext4`, `cp`, `fallocate`, `debugfs`,
      `e2fsck` on PATH.
- A boot-scoped sentinel under `/run/m80-preflight-ok-<sha256>` caches only
  the two expensive immutable-artifact checks: `firecracker --version` and
  `m80-image-manifest::verify`. The key includes the kernel boot id, configured
  Firecracker version pin, kernel kind override, and metadata for the
  Firecracker, jailer, hardening wrapper, kernel, rootfs, and manifest files.
  Corrupt or mismatched sentinels are ignored and rewritten after a successful
  full check. `M80_FORCE_PREFLIGHT=1` disables the cache for that invocation.
- Env keys are exact and case-sensitive. Preflight recognizes
  `M80_FIRECRACKER_BIN`, `M80_FIRECRACKER_VERSION`, `M80_JAILER_BIN`,
  `M80_JAILER_HARDEN_BIN`, `M80_KERNEL_IMAGE`, `M80_ARTIFACT_DIR`,
  `M80_ROOTFS_IMAGE`, `M80_KERNEL_KIND`, `M80_RUN_ROOT`, and
  `M80_FORCE_PREFLIGHT`. `M80_SKIP_CHECK_VULNERABILITIES=1` is a documented
  escape hatch for the CPU vulnerability hard gate. The full schema is captured
  in `docs/behaviors/configuration/env-schema.md`.
- Each check produces a row in the `Discovery::report` field. The same
  data is rendered as a fixed-width table for human consumption via
  `Discovery::render_table()`.
- Errors carry "what to try next" hints. For example,
  `KvmUnavailable { path }` names the missing device, `KvmNotWritable { path }`
  includes the suggestion to add the user to the `kvm` group, and
  `FirecrackerVersionMismatch` includes both the expected and actual versions
  plus the env var to override.

## Public surface

- `run() -> Result<Discovery, PreflightError>`.
- `run_with_configs(binary_config: BinaryDiscoveryConfig, artifact_config: ArtifactPreflightConfig, host_feature_config: HostFeaturePreflightConfig) -> Result<Discovery, PreflightError>` — composable entry point that accepts pre-built config structs rather than reading env vars internally.
- `BinaryDiscoveryConfig { firecracker_bin, jailer_bin, jailer_harden_bin,
  expected_firecracker_version }` and `BinaryDiscoveryConfig::from_env()` for
  explicit `run_with_configs` callers. There is no `Default`; callers must use
  `from_env()` or construct the full effective config.
- `ArtifactPreflightConfig { kernel_image, artifact_dir, rootfs_image,
  kernel_kind, run_root, helper_search_path }` and
  `ArtifactPreflightConfig::from_env()` for explicit `run_with_configs`
  callers. There is no `Default`; callers must use `from_env()` or construct
  the full effective config.
- `HostFeaturePreflightConfig { cgroup_mode }` and
  `HostFeaturePreflightConfig::from_env() -> Result<Self, PreflightError>`,
  plus `CgroupPreflightMode { UnifiedV2, Disabled }`, for checks whose
  required host features depend on effective config.
- `classify_privilege(euid, effective_caps) -> Result<PrivilegeStatus,
  PreflightError>` — pure classifier used by the live privilege probe and
  focused tests.
- `Discovery { firecracker_bin, jailer_bin, jailer_harden_bin, kernel: PathBuf,
  rootfs: PathBuf, manifest: m80_image_manifest::Manifest, run_root: PathBuf,
  privilege: PrivilegeStatus, report: Vec<CheckRow> }`.
- `Discovery::render_table()` → `String`.
- `CheckRow { label, passed, detail }`.
- `PrivilegeStatus { Root, CapabilityBearing }`.
- `REQUIRED_CAPABILITIES: &[caps::Capability]` — the per-call cap list
  (`CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, `CAP_MKNOD`, `CAP_CHOWN`,
  `CAP_FOWNER`, `CAP_KILL`).
- `ENV_FIRECRACKER_BIN`, `ENV_FIRECRACKER_VERSION`,
  `DEFAULT_FIRECRACKER_BIN`, `ENV_KERNEL_IMAGE`, `ENV_KERNEL_KIND`, and
  `ENV_ROOTFS_IMAGE` — shared keys/default used by the CLI when it displays or
  overlays effective preflight inputs. Other env keys/default paths remain
  crate-private implementation details of `from_env()`.
- `PreflightError`: `UnsupportedHostPlatform { actual }`,
  `KvmUnavailable { path }`, `KvmNotWritable { path }`,
  `KvmCpuExtensionMissing`, `InvalidCgroupMode { actual }`,
  `CpuVulnerabilityDetected { id, detail }`,
  `CgroupV2Unavailable`, `VsockUnavailable`, `TunUnavailable`,
  `NfConntrackUnavailable`,
  `KernelModulesMissing { missing: Vec<String> }`,
  `PrivilegeUnavailable { missing_caps: Vec<caps::Capability> }`,
  `FirecrackerBinaryNotFound`,
  `FirecrackerVersionMismatch { expected, actual }`,
  `FirecrackerCveFloorViolation { cve_id, actual, fixed_versions }`,
  `CapabilityRead(caps::errors::CapsError)` (failed to read the process's
  effective capability set),
  `JailerBinaryNotFound`, `JailerHardenBinaryNotFound`, `NonAbsolutePath { kind, path }`,
  `KernelNotFound`, `RootfsNotFound`, `Manifest(m80_image_manifest::ManifestError)`,
  `RunRootUnavailable { reason: String }` (covers both missing-dir and
  insufficient-space), `StorageHelperMissing(String)`, `Io(io::Error)`.
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
- Sentinel cache: corrupt sentinel rewrite, boot-id invalidation, and rootfs
  metadata invalidation.
