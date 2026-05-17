//! BootSpec YAML/JSON parser for layered-rootfs and warm-template config.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use crate::{
    ConfigError, ErofsImageRef, FcError, GuestMountPath, HookSpec, HookSpecSet, HostnameSpec,
    ImageDigest, NetworkPolicy, PmemLayer, PmemSharing, TemplateFingerprint, TrustDomainAck,
    TrustReason,
};

/// Parsed BootSpec configuration.
#[derive(Debug, Clone)]
pub struct BootSpec {
    /// BootSpec schema version.
    pub schema_version: u32,
    /// Optional human-facing name for CLI output and template-build logs.
    pub name: Option<String>,
    /// Sandbox resource and policy settings.
    pub sandbox: BootSpecSandbox,
    /// Typed read-only pmem layers.
    pub pmem_layers: Vec<PmemLayer>,
    /// Warm-fill or snapshot-template strategy requested by the file.
    pub warm_strategy: BootSpecWarmStrategy,
}

/// Sandbox settings parsed from a BootSpec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootSpecSandbox {
    /// VM id prefix used by callers that derive concrete VM ids later.
    pub vm_id_prefix: String,
    /// Requested vCPU count.
    pub vcpu_count: u32,
    /// Requested memory size in MiB.
    pub mem_size_mib: u32,
    /// Optional host workspace path.
    pub workspace: Option<PathBuf>,
    /// Requested network policy.
    pub network: NetworkPolicy,
    /// Writable overlay size in bytes.
    pub overlay_size_bytes: u64,
    /// Caller boot-argument tokens admitted as append-only extras.
    pub boot_args: Vec<String>,
}

/// Warm strategy parsed from a BootSpec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootSpecWarmStrategy {
    /// Fill slots by normal boot.
    BootFill,
    /// Fill slots by restoring a snapshot-template body.
    SnapshotRestore {
        /// Template store root path from the BootSpec.
        template_store: PathBuf,
        /// Template fingerprint requested by the BootSpec.
        template_fingerprint: TemplateFingerprint,
        /// Ready probe that must pass before hand-back.
        ready_probe: BootSpecReadyProbe,
        /// Closed typed post-restore hook set.
        hooks: HookSpecSet,
    },
}

/// Ready probe parsed from a snapshot-restore BootSpec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootSpecReadyProbe {
    /// Guest program path to execute.
    pub program: String,
    /// Guest program arguments.
    pub args: Vec<String>,
    /// Probe timeout in milliseconds.
    pub timeout_ms: u64,
}

/// Load a BootSpec from YAML text.
pub fn load_boot_spec_yaml_str(input: &str) -> Result<BootSpec, FcError> {
    let raw: RawBootSpec = serde_yaml::from_str(input).map_err(|source| {
        FcError::Config(ConfigError::InvalidValue {
            field: "boot_spec.yaml",
            reason: source.to_string(),
        })
    })?;
    BootSpec::from_raw(raw)
}

/// Load a BootSpec from JSON text.
pub fn load_boot_spec_json_str(input: &str) -> Result<BootSpec, FcError> {
    let raw: RawBootSpec = serde_json::from_str(input).map_err(|source| {
        FcError::Config(ConfigError::InvalidValue {
            field: "boot_spec.json",
            reason: source.to_string(),
        })
    })?;
    BootSpec::from_raw(raw)
}

impl BootSpec {
    fn from_raw(raw: RawBootSpec) -> Result<Self, FcError> {
        if raw.schema_version != 1 {
            return invalid_value("schema_version", "only schema_version 1 is supported");
        }
        let sandbox = parse_sandbox(raw.sandbox)?;
        let pmem_layers = raw
            .pmem_layers
            .into_iter()
            .map(parse_pmem_layer)
            .collect::<Result<Vec<_>, _>>()?;
        crate::validate_pmem_layers(&pmem_layers)?;
        let warm_strategy = parse_warm_strategy(raw.warm_strategy)?;
        Ok(Self {
            schema_version: raw.schema_version,
            name: raw.name,
            sandbox,
            pmem_layers,
            warm_strategy,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBootSpec {
    schema_version: u32,
    #[serde(default)]
    name: Option<String>,
    sandbox: RawSandbox,
    #[serde(default)]
    pmem_layers: Vec<RawPmemLayer>,
    warm_strategy: RawWarmStrategy,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSandbox {
    vm_id_prefix: String,
    vcpu_count: u32,
    mem_size_mib: u32,
    #[serde(default)]
    workspace: Option<PathBuf>,
    network: RawNetwork,
    overlay_size_bytes: u64,
    #[serde(default)]
    boot_args: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawNetwork {
    None,
    Outbound,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPmemLayer {
    image: RawImage,
    mount_at: String,
    sharing: RawSharing,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawImage {
    digest: String,
    format: RawImageFormat,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawImageFormat {
    Erofs,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSharing {
    mode: RawSharingMode,
    #[serde(default)]
    trust_reason: Option<RawTrustReason>,
    #[serde(default)]
    acknowledged: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawSharingMode {
    PerVm,
    Shared,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawTrustReason {
    SameOperator,
    KubernetesSameNamespace,
    ResearchSandbox,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWarmStrategy {
    mode: RawWarmStrategyMode,
    #[serde(default)]
    template: Option<RawTemplateRef>,
    #[serde(default)]
    ready_probe: Option<RawReadyProbe>,
    #[serde(default)]
    hooks: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawWarmStrategyMode {
    BootFill,
    SnapshotRestore,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTemplateRef {
    store: PathBuf,
    fingerprint: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReadyProbe {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    timeout_ms: u64,
}

fn parse_sandbox(raw: RawSandbox) -> Result<BootSpecSandbox, FcError> {
    validate_non_empty("sandbox.vm_id_prefix", &raw.vm_id_prefix)?;
    if raw.vcpu_count == 0 {
        return invalid_value("sandbox.vcpu_count", "must be greater than zero");
    }
    if raw.mem_size_mib == 0 {
        return invalid_value("sandbox.mem_size_mib", "must be greater than zero");
    }
    if raw.overlay_size_bytes == 0 {
        return invalid_value("sandbox.overlay_size_bytes", "must be greater than zero");
    }
    validate_boot_args(&raw.boot_args)?;
    Ok(BootSpecSandbox {
        vm_id_prefix: raw.vm_id_prefix,
        vcpu_count: raw.vcpu_count,
        mem_size_mib: raw.mem_size_mib,
        workspace: raw.workspace,
        network: match raw.network {
            RawNetwork::None => NetworkPolicy::NoEgress,
            RawNetwork::Outbound => NetworkPolicy::AllowOutbound {
                exceptions: Vec::new(),
            },
        },
        overlay_size_bytes: raw.overlay_size_bytes,
        boot_args: raw.boot_args,
    })
}

fn parse_pmem_layer(raw: RawPmemLayer) -> Result<PmemLayer, FcError> {
    let RawImageFormat::Erofs = raw.image.format;
    let digest = parse_image_digest(&raw.image.digest, "pmem_layers[].image.digest")?;
    let image = ErofsImageRef::from_digest(digest);
    let mount_at = parse_guest_mount_path(&raw.mount_at, "pmem_layers[].mount_at")?;
    let sharing = parse_sharing(raw.sharing)?;
    Ok(PmemLayer::new(image, sharing, mount_at))
}

fn parse_sharing(raw: RawSharing) -> Result<PmemSharing, FcError> {
    match raw.mode {
        RawSharingMode::PerVm => {
            if raw.trust_reason.is_some() || raw.acknowledged.is_some() {
                return invalid_value(
                    "pmem_layers[].sharing",
                    "per_vm sharing must not carry trust_reason or acknowledged",
                );
            }
            Ok(PmemSharing::PerVm)
        }
        RawSharingMode::Shared => {
            let reason = raw
                .trust_reason
                .ok_or(FcError::Config(ConfigError::MissingField {
                    field: "pmem_layers[].sharing.trust_reason",
                }))?;
            if raw.acknowledged != Some(true) {
                return invalid_value(
                    "pmem_layers[].sharing.acknowledged",
                    "shared pmem requires acknowledged: true",
                );
            }
            Ok(PmemSharing::Shared(TrustDomainAck::new(match reason {
                RawTrustReason::SameOperator => TrustReason::SameOperator,
                RawTrustReason::KubernetesSameNamespace => TrustReason::KubernetesSameNamespace,
                RawTrustReason::ResearchSandbox => TrustReason::ResearchSandbox,
            })))
        }
    }
}

fn parse_warm_strategy(raw: RawWarmStrategy) -> Result<BootSpecWarmStrategy, FcError> {
    match raw.mode {
        RawWarmStrategyMode::BootFill => {
            if raw.template.is_some() || raw.ready_probe.is_some() || !raw.hooks.is_empty() {
                return invalid_value(
                    "warm_strategy",
                    "boot_fill must not include template, ready_probe, or hooks",
                );
            }
            Ok(BootSpecWarmStrategy::BootFill)
        }
        RawWarmStrategyMode::SnapshotRestore => {
            let template = raw
                .template
                .ok_or(FcError::Config(ConfigError::MissingField {
                    field: "warm_strategy.template",
                }))?;
            let ready_probe =
                raw.ready_probe
                    .ok_or(FcError::Config(ConfigError::MissingField {
                        field: "warm_strategy.ready_probe",
                    }))?;
            Ok(BootSpecWarmStrategy::SnapshotRestore {
                template_store: template.store,
                template_fingerprint: parse_template_fingerprint(
                    &template.fingerprint,
                    "warm_strategy.template.fingerprint",
                )?,
                ready_probe: parse_ready_probe(ready_probe)?,
                hooks: HookSpecSet::new(parse_hooks(raw.hooks)?),
            })
        }
    }
}

fn parse_ready_probe(raw: RawReadyProbe) -> Result<BootSpecReadyProbe, FcError> {
    validate_non_empty("warm_strategy.ready_probe.program", &raw.program)?;
    if raw.timeout_ms == 0 {
        return invalid_value(
            "warm_strategy.ready_probe.timeout_ms",
            "must be greater than zero",
        );
    }
    Ok(BootSpecReadyProbe {
        program: raw.program,
        args: raw.args,
        timeout_ms: raw.timeout_ms,
    })
}

fn parse_hooks(raw_hooks: Vec<Value>) -> Result<Vec<HookSpec>, FcError> {
    raw_hooks.into_iter().map(parse_hook).collect()
}

fn parse_hook(value: Value) -> Result<HookSpec, FcError> {
    match value {
        Value::String(name) => match name.as_str() {
            "reseed_systemd_random_seed" => Ok(HookSpec::ReseedSystemdRandomSeed),
            "regen_machine_id" => Ok(HookSpec::RegenMachineId),
            _ => invalid_value("warm_strategy.hooks[].variant", "unknown HookSpec variant"),
        },
        Value::Object(object) => {
            if object.len() != 1 {
                return invalid_value(
                    "warm_strategy.hooks[]",
                    "hook object must contain exactly one variant",
                );
            }
            let (variant, payload) = object.into_iter().next().expect("one hook entry");
            match variant.as_str() {
                "set_hostname" => parse_set_hostname_hook(payload),
                _ => invalid_value("warm_strategy.hooks[].variant", "unknown HookSpec variant"),
            }
        }
        _ => invalid_value(
            "warm_strategy.hooks[]",
            "hook must be a string variant or single-entry object",
        ),
    }
}

fn parse_set_hostname_hook(payload: Value) -> Result<HookSpec, FcError> {
    let object = payload.as_object().ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "warm_strategy.hooks[].set_hostname",
            reason: "set_hostname payload must be an object".to_owned(),
        })
    })?;
    let allowed = ["hostname"].into_iter().collect::<Vec<_>>();
    reject_unknown_keys(
        "warm_strategy.hooks[].set_hostname",
        object.keys().map(String::as_str),
        &allowed,
    )?;
    let hostname = object
        .get("hostname")
        .and_then(Value::as_str)
        .ok_or(FcError::Config(ConfigError::MissingField {
            field: "warm_strategy.hooks[].set_hostname.hostname",
        }))?;
    let hostname = HostnameSpec::new(hostname).map_err(|source| {
        FcError::Config(ConfigError::InvalidValue {
            field: "warm_strategy.hooks[].set_hostname.hostname",
            reason: source.to_string(),
        })
    })?;
    Ok(HookSpec::SetHostname(hostname))
}

fn parse_image_digest(value: &str, field: &'static str) -> Result<ImageDigest, FcError> {
    let digest = value.strip_prefix("sha256:").unwrap_or(value);
    ImageDigest::parse(digest).map_err(|source| {
        FcError::Config(ConfigError::InvalidValue {
            field,
            reason: source.to_string(),
        })
    })
}

fn parse_template_fingerprint(
    value: &str,
    field: &'static str,
) -> Result<TemplateFingerprint, FcError> {
    let fingerprint = value.strip_prefix("sha256:").unwrap_or(value);
    TemplateFingerprint::parse_hex(fingerprint).map_err(|source| {
        FcError::Config(ConfigError::InvalidValue {
            field,
            reason: source.to_string(),
        })
    })
}

fn parse_guest_mount_path(value: &str, field: &'static str) -> Result<GuestMountPath, FcError> {
    GuestMountPath::parse(value).map_err(|source| {
        FcError::Config(ConfigError::InvalidValue {
            field,
            reason: source.to_string(),
        })
    })
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), FcError> {
    if value.is_empty() {
        return invalid_value(field, "must not be empty");
    }
    Ok(())
}

fn validate_boot_args(tokens: &[String]) -> Result<(), FcError> {
    for token in tokens {
        if token.is_empty() {
            return invalid_value("sandbox.boot_args", "boot arg token must not be empty");
        }
        if token.chars().any(char::is_whitespace) || token.chars().any(char::is_control) {
            return invalid_value(
                "sandbox.boot_args",
                "boot arg tokens must not contain whitespace or control characters",
            );
        }
        if is_m80_owned_kernel_arg(token) {
            return invalid_value("sandbox.boot_args", "boot arg overrides an m80-owned key");
        }
    }
    Ok(())
}

fn is_m80_owned_kernel_arg(token: &str) -> bool {
    ["init=", "m80.workspace=", "m80.rootfs=", "rootfstype="]
        .iter()
        .any(|prefix| token.starts_with(prefix))
}

fn reject_unknown_keys<I>(field: &'static str, keys: I, allowed: &[&str]) -> Result<(), FcError>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    for key in keys {
        if !allowed.contains(&key.as_ref()) {
            return invalid_value(field, "unknown field");
        }
    }
    Ok(())
}

fn invalid_value<T>(field: &'static str, reason: impl Into<String>) -> Result<T, FcError> {
    Err(FcError::Config(ConfigError::InvalidValue {
        field,
        reason: reason.into(),
    }))
}

#[cfg(test)]
mod tests;
