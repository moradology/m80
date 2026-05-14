# Workspace And JoinNetns Admission

Behavior capture for bead `m80-8emae.40`.

## Workspace Root

`Backend::admit` canonicalizes `SandboxConfig::workspace` before the sandbox is
created. The workspace root itself must be a real directory, not a symlink. The
canonical path is stored in the admitted `SandboxConfig` and later passed to
`m80-storage` for scratch hydration.

This is an admission shape check, not workspace authorization. m80 still accepts
a caller-selected real directory; adapters decide whether that directory is
allowed before launch.

This complements storage-level tree admissibility. Storage still rejects
symlinks and special files inside the workspace tree and opens regular files
with `O_NOFOLLOW` at copy time.

## JoinNetns DNS

`Backend::admit` validates every `NetworkPolicy::JoinNetns` DNS resolver with
the same `is_admitted_dns_resolver` predicate used by OutboundNat. A rejected
resolver fails as `FcError::Config(ConfigError::InvalidValue)` before an
admission permit is consumed.

## Verification

- `crates/m80-firecracker/src/backend.rs::tests::admit_canonicalizes_workspace_root_before_sandbox_creation`
- `crates/m80-firecracker/src/backend.rs::tests::admit_rejects_symlink_workspace_root`
- `crates/m80-firecracker/src/backend.rs::tests::admit_rejects_join_netns_unadmitted_dns_resolver`
- `crates/m80-firecracker/src/backend.rs::tests::admit_accepts_join_netns_admitted_dns_resolver`
