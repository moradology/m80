use super::*;

const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const FINGERPRINT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[test]
fn yaml_loader_parses_typed_pmem_and_snapshot_restore() {
    let spec = load_boot_spec_yaml_str(&snapshot_yaml()).expect("valid boot spec");

    assert_eq!(spec.schema_version, 1);
    assert_eq!(spec.name.as_deref(), Some("typed"));
    assert_eq!(
        spec.sandbox.overlay_clone_mode,
        OverlayTemplateCloneMode::ByteCopy
    );
    assert_eq!(spec.pmem_layers.len(), 2);
    assert_eq!(spec.pmem_layers[0].image().digest().as_str(), DIGEST_A);
    assert!(matches!(spec.pmem_layers[0].sharing(), PmemSharing::PerVm));
    assert!(matches!(
        spec.pmem_layers[1].sharing(),
        PmemSharing::Shared(_)
    ));
    let BootSpecWarmStrategy::SnapshotRestore {
        template_fingerprint,
        hooks,
        ready_probe,
        ..
    } = spec.warm_strategy
    else {
        panic!("expected snapshot restore");
    };
    assert_eq!(template_fingerprint.to_hex(), FINGERPRINT);
    assert_eq!(hooks.hooks().len(), 3);
    assert_eq!(ready_probe.program, "/bin/true");
}

#[test]
fn json_loader_shares_validation_path() {
    let spec = load_boot_spec_json_str(&valid_json()).expect("valid json boot spec");
    assert_eq!(spec.schema_version, 1);
    assert_eq!(
        spec.sandbox.overlay_clone_mode,
        OverlayTemplateCloneMode::Auto
    );
    assert!(matches!(spec.warm_strategy, BootSpecWarmStrategy::BootFill));
    assert_eq!(spec.pmem_layers.len(), 1);
}

#[test]
fn bad_digest_fails_typed_before_host_action() {
    let yaml = valid_yaml().replace(&format!("sha256:{DIGEST_A}"), "sha256:not-hex");
    let err = load_boot_spec_yaml_str(&yaml).unwrap_err();
    assert!(matches!(
        err,
        FcError::Config(ConfigError::InvalidValue {
            field: "pmem_layers[].image.digest",
            ..
        })
    ));
}

#[test]
fn mount_path_escape_fails_typed_before_host_action() {
    let yaml = valid_yaml().replace("/opt/m80-layers/toolchain", "/opt/m80-layers/..");
    let err = load_boot_spec_yaml_str(&yaml).unwrap_err();
    assert!(matches!(
        err,
        FcError::Config(ConfigError::InvalidValue {
            field: "pmem_layers[].mount_at",
            ..
        })
    ));
}

#[test]
fn shared_without_trust_reason_fails_typed_before_host_action() {
    let yaml = valid_yaml().replace("      trust_reason: same_operator\n", "");
    let err = load_boot_spec_yaml_str(&yaml).unwrap_err();
    assert!(matches!(
        err,
        FcError::Config(ConfigError::MissingField {
            field: "pmem_layers[].sharing.trust_reason"
        })
    ));
}

#[test]
fn invalid_hostname_fails_typed_before_host_action() {
    let yaml = snapshot_yaml().replace("hostname: m80-template", "hostname: -bad-host");
    let err = load_boot_spec_yaml_str(&yaml).unwrap_err();
    assert!(matches!(
        err,
        FcError::Config(ConfigError::InvalidValue {
            field: "warm_strategy.hooks[].set_hostname.hostname",
            ..
        })
    ));
}

#[test]
fn unknown_hook_variant_fails_typed_before_host_action() {
    let yaml = snapshot_yaml().replace("reseed_systemd_random_seed", "run_user_cmd");
    let err = load_boot_spec_yaml_str(&yaml).unwrap_err();
    assert!(matches!(
        err,
        FcError::Config(ConfigError::InvalidValue {
            field: "warm_strategy.hooks[].variant",
            ..
        })
    ));
}

#[test]
fn unknown_boot_spec_fields_fail_closed() {
    let yaml = valid_yaml().replace("schema_version: 1", "schema_version: 1\nunknown: true");
    let err = load_boot_spec_yaml_str(&yaml).unwrap_err();
    assert!(matches!(
        err,
        FcError::Config(ConfigError::InvalidValue {
            field: "boot_spec.yaml",
            ..
        })
    ));
}

#[test]
fn committed_yaml_examples_parse() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let examples = manifest_dir.join("../../docs/examples");
    for name in [
        "boot-spec-minimal.yaml",
        "boot-spec-pervm-pmem.yaml",
        "boot-spec-mixed-pmem.yaml",
        "boot-spec-snapshot-restore.yaml",
        "boot-spec-full.yaml",
    ] {
        let path = examples.join(name);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!("failed to read {}: {err}", path.display());
        });
        load_boot_spec_yaml_str(&text).unwrap_or_else(|err| {
            panic!("failed to parse {}: {err:?}", path.display());
        });
    }
}

#[test]
fn boot_args_reject_m80_owned_kernel_tokens() {
    let yaml = valid_yaml().replace("boot_args: []", "boot_args:\n  - init=/bin/sh");
    let err = load_boot_spec_yaml_str(&yaml).unwrap_err();
    assert!(matches!(
        err,
        FcError::Config(ConfigError::InvalidValue {
            field: "sandbox.boot_args",
            ..
        })
    ));
}

fn valid_yaml() -> String {
    format!(
        r#"schema_version: 1
name: typed
sandbox:
  vm_id_prefix: m80-typed
  vcpu_count: 1
  mem_size_mib: 512
  workspace: null
  network: none
  overlay_size_bytes: 134217728
  overlay_clone_mode: byte_copy
  boot_args: []
pmem_layers:
  - image:
      digest: sha256:{DIGEST_A}
      format: erofs
    mount_at: /opt/m80-layers/toolchain
    sharing:
      mode: shared
      trust_reason: same_operator
      acknowledged: true
warm_strategy:
  mode: boot_fill
"#
    )
}

fn snapshot_yaml() -> String {
    format!(
        r#"schema_version: 1
name: typed
sandbox:
  vm_id_prefix: m80-typed
  vcpu_count: 1
  mem_size_mib: 512
  workspace: null
  network: none
  overlay_size_bytes: 134217728
  overlay_clone_mode: byte_copy
  boot_args: []
pmem_layers:
  - image:
      digest: sha256:{DIGEST_A}
      format: erofs
    mount_at: /opt/m80-layers/project
    sharing:
      mode: per_vm
  - image:
      digest: sha256:{DIGEST_B}
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
    fingerprint: {FINGERPRINT}
  ready_probe:
    program: /bin/true
    args: []
    timeout_ms: 5000
  hooks:
    - reseed_systemd_random_seed
    - regen_machine_id
    - set_hostname:
        hostname: m80-template
"#
    )
}

fn valid_json() -> String {
    format!(
        r#"{{
  "schema_version": 1,
  "name": "typed-json",
  "sandbox": {{
    "vm_id_prefix": "m80-json",
    "vcpu_count": 1,
    "mem_size_mib": 512,
    "workspace": null,
    "network": "none",
    "overlay_size_bytes": 134217728,
    "overlay_clone_mode": "auto",
    "boot_args": []
  }},
  "pmem_layers": [
    {{
      "image": {{ "digest": "sha256:{DIGEST_A}", "format": "erofs" }},
      "mount_at": "/opt/m80-layers/toolchain",
      "sharing": {{ "mode": "per_vm" }}
    }}
  ],
  "warm_strategy": {{ "mode": "boot_fill" }}
}}"#
    )
}
