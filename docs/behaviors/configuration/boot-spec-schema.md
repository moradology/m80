# BootSpec Config Schema

Bead: `m80-q420k.5.3`.

BootSpec is the Phase E YAML/JSON config surface for layered rootfs and
snapshot-template warm restore. It is parsed by `m80-firecracker` without
touching the host substrate.

## Loaders

- `m80_firecracker::load_boot_spec_yaml_str(&str) -> Result<BootSpec, FcError>`
- `m80_firecracker::load_boot_spec_json_str(&str) -> Result<BootSpec, FcError>`

Both loaders share one typed validation path after serde. Unknown fields fail
closed because every raw serde struct uses `deny_unknown_fields`.

## Shape

```yaml
schema_version: 1
name: full-layered-template

sandbox:
  vm_id_prefix: m80-full
  vcpu_count: 1
  mem_size_mib: 512
  workspace: null
  network: none
  overlay_size_bytes: 134217728
  overlay_clone_mode: byte_copy
  boot_args: []

pmem_layers:
  - image:
      digest: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
      format: erofs
    mount_at: /opt/m80-layers/toolchain
    sharing:
      mode: shared
      trust_reason: same_operator
      acknowledged: true

warm_strategy:
  mode: snapshot_restore
  template:
    store: /var/lib/m80/templates
    fingerprint: cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
  ready_probe:
    program: /bin/true
    args: []
    timeout_ms: 5000
  hooks:
    - reseed_systemd_random_seed
    - regen_machine_id
    - set_hostname:
        hostname: m80-template
```

See `docs/examples/` for the committed parser fixtures.

## Typed Construction

The parser constructs existing Rust types before returning `BootSpec`:

- `ImageDigest` and `ErofsImageRef`.
- `GuestMountPath`.
- `PmemSharing::PerVm`.
- `PmemSharing::Shared(TrustDomainAck)` from a finite `TrustReason`.
- `TemplateFingerprint`.
- `HookSpecSet`, `HookSpec`, and `HostnameSpec`.

This keeps validation at the typed API boundary instead of at the final host
consumer. The parse path does not create a jailer plan, start Firecracker,
attach pmem, build images, mutate stores, or restore templates.

## Failure Cases

The parser returns `FcError::Config(ConfigError::...)` for the named bad config
cases:

- `pmem_layers[].image.digest`: invalid sha256-shaped digest.
- `pmem_layers[].mount_at`: guest mount path escape, reserved mount shadowing,
  or invalid layer name.
- `pmem_layers[].sharing.trust_reason`: `shared` mode without a trust-domain
  reason.
- `warm_strategy.hooks[].set_hostname.hostname`: invalid RFC-1123 hostname.
- `warm_strategy.hooks[].variant`: unknown hook variant.

Boot args are also rejected if a token contains whitespace/control characters
or attempts to override m80-owned kernel arguments.

## Verification

- `crates/m80-firecracker/src/boot_spec/tests.rs::yaml_loader_parses_typed_pmem_and_snapshot_restore`
- `crates/m80-firecracker/src/boot_spec/tests.rs::json_loader_shares_validation_path`
- `crates/m80-firecracker/src/boot_spec/tests.rs::bad_digest_fails_typed_before_host_action`
- `crates/m80-firecracker/src/boot_spec/tests.rs::mount_path_escape_fails_typed_before_host_action`
- `crates/m80-firecracker/src/boot_spec/tests.rs::shared_without_trust_reason_fails_typed_before_host_action`
- `crates/m80-firecracker/src/boot_spec/tests.rs::invalid_hostname_fails_typed_before_host_action`
- `crates/m80-firecracker/src/boot_spec/tests.rs::unknown_hook_variant_fails_typed_before_host_action`
- `crates/m80-firecracker/src/boot_spec/tests.rs::committed_yaml_examples_parse`
