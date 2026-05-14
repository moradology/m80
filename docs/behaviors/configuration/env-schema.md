# Environment Variable Schema

## Contract

m80 has two environment-variable surfaces:

- **Preflight discovery** resolves host binaries and boot artifacts before any VM
  is admitted.
- **Orchestrator configuration** resolves backend settings that can also appear
  in `config.toml` and CLI flag overrides.

All keys are exact and case-sensitive. Legacy predecessor-style names such as
`M80_FIRECRACKER_ROOTFS_IMAGE`, `M80_FIRECRACKER_RUN_ROOT`,
`M80_FIRECRACKER_MAX_CONCURRENT_VMS`, `M80_VERSION`, and
`M80_JAILER_MODE` are not accepted aliases.

## Preflight Discovery

| Env var | Meaning | Default when unset |
|---|---|---|
| `M80_FIRECRACKER_BIN` | Host path to the `firecracker` binary. | `/opt/firecracker/bin/firecracker` |
| `M80_FIRECRACKER_VERSION` | Optional exact version pin compared to `firecracker --version`. | no version comparison |
| `M80_JAILER_BIN` | Host path to the `jailer` binary. | `/opt/firecracker/bin/jailer` |
| `M80_JAILER_HARDEN_BIN` | Host path to the m80 hardening wrapper that execs the official jailer. | `/opt/m80/bin/m80-jailer-harden` |
| `M80_KERNEL_IMAGE` | Host path to the guest kernel image. | latest `vmlinux-*` under `M80_ARTIFACT_DIR` |
| `M80_ARTIFACT_DIR` | Directory searched for `vmlinux-*` when `M80_KERNEL_IMAGE` is unset. | `/opt/m80/artifacts` |
| `M80_ROOTFS_IMAGE` | Host path to the built ext4 rootfs image. | none; missing rootfs fails preflight |
| `M80_KERNEL_KIND` | `stock` or `stripped`; overrides the manifest kernel-kind discriminator after reading the rootfs manifest. | manifest value |
| `M80_RUN_ROOT` | Host run-root directory used for VM state. | `/var/run/m80` |
| `M80_JAIL_UID` | UID that must exist for the jailed Firecracker process. Shared with orchestrator config. | `3000` |
| `M80_JAIL_GID` | GID that must exist for the jailed Firecracker process. Shared with orchestrator config. | `3000` |
| `M80_CGROUP_MODE` | `unified-v2` or `disabled`. Shared with orchestrator config. | `unified-v2` |
| `M80_MAX_CONCURRENT_VMS` | Expected concurrent VM count used to size `net.netfilter.nf_conntrack_max`; also the orchestrator admission semaphore size. | `8` |
| `M80_FORCE_PREFLIGHT` | When set to any value, bypasses the boot-scoped preflight sentinel cache and reruns Firecracker version probing plus manifest SHA verification. | cache enabled |
| `M80_SKIP_CHECK_VULNERABILITIES` | When set exactly to `1`, skips hard failure for CPU vulnerability sysfs rows such as `mds` and `l1tf`. | vulnerability gate enabled |

Preflight does not create missing directories, install binaries, download
artifacts, or fall back to a different image after a bad env value. The first
failed check returns a typed `PreflightError`.

## Orchestrator Config

| Env var | Config field | Meaning | Default |
|---|---|---|---|
| `M80_DEFAULT_PROFILE` | `default_profile` | Local runtime profile used by `m80 run` when `--profile` is absent. | `env` |
| `M80_MAX_CONCURRENT_VMS` | `max_concurrent_vms` | Admission semaphore size for concurrently running VMs. | `8` |
| `M80_RUN_ROOT` | `run_root` | Run-root directory. Shared with preflight discovery. | `/var/run/m80` |
| `M80_JAIL_UID` | `jail_uid` | UID for the jailed Firecracker process. | `3000` |
| `M80_JAIL_GID` | `jail_gid` | GID for the jailed Firecracker process. | `3000` |
| `M80_CGROUP_MODE` | `cgroup_mode` | `unified-v2` or `disabled`. | `unified-v2` |

Only the fields above are valid in this loader. Unknown config-file keys,
unknown config.d drop-in keys, and unknown flag-override keys fail closed with
`FcError::Config`; they are not silently ignored.

## Trace And Bench Env Vars

`M80_PHASE_TRACE`, `M80_GUEST_BOOT`, `M80_*_BENCH_*`, and
`M80_WARM_POOL_READY` are diagnostics or benchmark controls. They are not part
of the configuration/discovery schema, and they do not affect the effective
backend config shown by `m80 config show`.

## Evidence

- `crates/m80-firecracker/tests/config_loading.rs::every_documented_env_key_maps_to_its_field`
  covers the orchestrator env keys without host KVM access.
- `crates/m80-firecracker/tests/config_loading.rs::unknown_toml_key_fails_closed`
  and `::unknown_flag_override_fails_closed` pin the fail-closed behavior.
- `crates/m80-preflight/src/checks.rs::tests::rootfs_and_manifest_check_honors_kernel_kind_env`
  covers `M80_KERNEL_KIND`.
- `crates/m80-preflight/src/checks.rs::tests::invalid_kernel_kind_env_fails_closed`
  covers invalid kernel-kind rejection.
- `crates/m80-preflight/src/cache.rs::tests::force_preflight_env_disables_cache_reads_and_writes`
  covers the `M80_FORCE_PREFLIGHT` cache bypass.
- `crates/m80-preflight/src/checks.rs::tests::cpu_vulnerability_scan_can_be_explicitly_skipped`
  covers the `M80_SKIP_CHECK_VULNERABILITIES=1` escape hatch.
