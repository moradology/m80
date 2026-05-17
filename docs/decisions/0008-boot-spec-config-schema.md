# 0008 - BootSpec Config Schema

## Context

Phase E of `m80-q420k` adds an operator-facing config shape for the mechanics
landed in Phases B-D: read-only erofs pmem layers, `PmemSharing::Shared`
trust-domain acknowledgement, snapshot-template restore, and closed
post-restore hooks.

The parser is pure config admission. It must fail before any host action such
as jailer plan construction, image-store mutation, Firecracker launch, pmem
attach, or snapshot restore. Kernel-facing values must be validated at typed
API boundaries before they become command-line tokens, guest mount paths, or
template identities.

## Decision

BootSpec supports YAML and JSON loaders:

```rust
load_boot_spec_yaml_str(text) -> Result<BootSpec, FcError>
load_boot_spec_json_str(text) -> Result<BootSpec, FcError>
```

Both loaders deserialize into the same raw schema and then run one typed
construction path. Serde structs use `#[serde(deny_unknown_fields)]`; unknown
top-level or nested fields fail closed.

Schema version `1` is the only accepted version. The v1 shape includes:

- `sandbox`: resource and policy settings such as `vm_id_prefix`,
  `vcpu_count`, `mem_size_mib`, `network`, `overlay_size_bytes`, and
  append-only `boot_args`.
- `pmem_layers`: a bounded list of erofs image digests, guest mount paths, and
  sharing mode.
- `warm_strategy`: either `boot_fill` or `snapshot_restore`.

Validation constructs the existing typed APIs:

- `ImageDigest` and `ErofsImageRef` for pmem image references.
- `GuestMountPath` for guest mount destinations.
- `PmemSharing::PerVm` or
  `PmemSharing::Shared(TrustDomainAck::new(TrustReason::*))`.
- `TemplateFingerprint` for snapshot-template identity.
- `HookSpecSet`, `HookSpec`, and `HostnameSpec` for post-restore hooks.

The config accepts `sha256:<hex>` or bare lowercase hex for digest-like input,
but typed Rust values store the existing normalized form. Shared pmem requires
both `trust_reason` and `acknowledged: true`; a shared config without the typed
witness is rejected.

Hook variants are closed:

- `reseed_systemd_random_seed`
- `regen_machine_id`
- `set_hostname: { hostname: ... }`

Unknown hook variants are rejected as `ConfigError::InvalidValue` before any
host action. Arbitrary post-restore commands remain out of scope.

## Consequences

Examples under `docs/examples/` are now parser fixtures with synthetic but
schema-valid digests and fingerprints. They are not promises that the Phase E
CLI commands are implemented; they pin the config shape for the later CLI and
e2e leaves.

Parse errors are typed `FcError::Config(ConfigError::...)` values. The five
named failure cases are pinned by unit tests:

- bad digest;
- mount-path escape or reserved mount shadowing;
- `sharing: shared` without `trust_reason`;
- invalid RFC-1123 hostname;
- unknown hook variant.

BootSpec `boot_args` are append-only tokens. They reject whitespace, control
characters, and m80-owned kernel keys such as `init=`, `m80.workspace=`,
`m80.rootfs=`, and `rootfstype=`.

## Alternatives Considered

**TOML only:** rejected. BootSpec is operator-facing and nested; YAML examples
are easier to read while JSON is useful for generated configs.

**Serde directly into production types:** rejected. Raw serde structs keep the
wire/config shape separate from typed construction, so validation remains at
the API boundary.

**Permit unknown fields for forward compatibility:** rejected. m80 owns its
consumers in v0.x; unknown fields are more likely stale assumptions than safe
future hints.

**Free-form hook commands:** rejected for the same reason as ADR 0007. Hooks
are lifecycle operations with privileged guest effects and must stay closed
typed variants.

## References

- `docs/examples/`
- `docs/behaviors/configuration/boot-spec-schema.md`
- `docs/decisions/0006-pmemlayer-api.md`
- `docs/decisions/0007-snapshot-template-lifecycle.md`
- `docs/positioning.md`
