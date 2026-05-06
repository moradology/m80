# `m80-preflight`

Pre-launch host validation: KVM, kernel modules, binaries, manifest,
storage helpers, run-root capacity. Emits a tabular report and a
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
  3. **Kernel modules** — `bridge` and `tap` loaded (read from
     `/proc/modules`). v0.1 does not attempt to load missing modules; the
     operator must `modprobe` them before running preflight.
  4. **Privilege** — `geteuid() == 0` OR the effective Linux capability set
     contains every entry in `REQUIRED_CAPABILITIES`
     (`CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, `CAP_MKNOD`, `CAP_CHOWN`,
     `CAP_FOWNER`, `CAP_KILL`). Probed via the `caps` crate against the
     process's effective set. Returns `PrivilegeStatus::Root` or
     `PrivilegeStatus::CapabilityBearing`.
  5. **Firecracker binary** — discovered via env override or default,
     `--version` matched against the configured pin.
  6. **Jailer binary** — same protocol.
  7. **Kernel artifact** — auto-discovered as the latest `vmlinux-*`
     under `<artifact_dir>`, or the env-overridden absolute path. When
     `M80_KERNEL_KIND=stock|stripped` is set, the discovered manifest's
     `kernel_kind` is overridden to match the selected kernel artifact.
  8. **Rootfs + manifest** — manifest schema validates,
     `m80-image-manifest::verify` recomputes every sha256. This is the
     boot-artifact trust boundary for `m80-firecracker`; launch phase 3 does
     not rehash these artifacts again for every VM.
  9. **Run-root** — absolute, must already exist, >= 100 MiB free
     (no silent creation; caller must ensure the directory is present).
  10. **Storage helpers** — `mkfs.ext4`, `cp`, `fallocate`, `debugfs`,
      `e2fsck` on PATH.
- Env keys are exact and case-sensitive. Preflight recognizes
  `M80_FIRECRACKER_BIN`, `M80_FIRECRACKER_VERSION`, `M80_JAILER_BIN`,
  `M80_KERNEL_IMAGE`, `M80_ARTIFACT_DIR`, `M80_ROOTFS_IMAGE`,
  `M80_KERNEL_KIND`, and `M80_RUN_ROOT`. The full schema is captured in
  `docs/behaviors/configuration/env-schema.md`.
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
- `Discovery { firecracker_bin, jailer_bin, kernel: PathBuf, rootfs: PathBuf,
  manifest: m80_image_manifest::Manifest, run_root: PathBuf, privilege: PrivilegeStatus,
  report: Vec<CheckRow> }`.
- `Discovery::render_table()` → `String`.
- `PrivilegeStatus { Root, CapabilityBearing }`.
- `REQUIRED_CAPABILITIES: &[caps::Capability]` — the per-call cap list
  (`CAP_NET_ADMIN`, `CAP_SYS_ADMIN`, `CAP_MKNOD`, `CAP_CHOWN`,
  `CAP_FOWNER`, `CAP_KILL`).
- `PreflightError`: `UnsupportedHostPlatform { actual }`,
  `KvmUnavailable { path }`, `KvmNotWritable { path }`,
  `KernelModulesMissing { missing: Vec<String> }`,
  `PrivilegeUnavailable { missing_caps: Vec<caps::Capability> }`,
  `FirecrackerBinaryNotFound`, `FirecrackerVersionMismatch { expected, actual }`,
  `JailerBinaryNotFound`, `KernelNotFound`, `RootfsNotFound`,
  `Manifest(m80_image_manifest::ManifestError)`,
  `RunRootUnavailable { reason: String }` (covers both missing-dir and
  insufficient-space), `StorageHelperMissing(String)`, `Io(io::Error)`.

## Non-goals

- **No remediation.** This crate reports; it does not chmod, chgrp,
  install packages, or download artifacts.
- **No image build.** That's `m80-image-build`. Preflight only verifies
  what's on disk.
- **No per-VM checks.** Capacity-per-VM, port availability, IP collision
  — those happen at boot time inside `m80-firecracker`, not here.
- **No daemon mode.** `run()` is a one-shot.

## Dependencies

- `m80-image-manifest` — for manifest validation.
- `caps` — for reading the process's effective Linux capability set.
- `serde`, `thiserror`, `tracing`, `nix`.

## Tests

- Happy path on a real KVM-capable host (CI behind a `kvm` feature flag).
- Each `PreflightError` variant has a dedicated negative test that
  arranges the host fixture to fail that check and asserts the typed
  error.
- Report determinism: same host state produces the same `report` rows
  in the same order.
- Hint coverage: every error variant carries a non-empty hint string.
