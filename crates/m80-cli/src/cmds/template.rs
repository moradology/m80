use std::collections::HashMap;
use std::path::{Path, PathBuf};

use m80_firecracker::{
    load_boot_spec_json_str, load_boot_spec_yaml_str, BootSpec, BootSpecWarmStrategy, ConfigError,
    FcError, SandboxConfig,
};
use m80_snapshot_template::{
    TemplateFingerprint, TemplateManifest, TemplateStore, TemplateSummary,
};
use serde::Serialize;

use crate::args::{
    TemplateAction, TemplateBuildArgs, TemplateFingerprintArgs, TemplatePruneArgs,
    TemplateStoreArgs,
};
use crate::{errors, json};

const DEFAULT_TEMPLATE_STORE_CAPACITY_FOR_READS: usize = 1024;
const DEFAULT_TEMPLATE_STORE_CAPACITY_FOR_BUILDS: usize = 1024;

pub(super) fn cmd_template(action: TemplateAction, json_mode: bool) -> anyhow::Result<i32> {
    match action {
        TemplateAction::Build(args) => cmd_build(args, json_mode),
        TemplateAction::List(args) => cmd_list(args, json_mode),
        TemplateAction::Show(args) => cmd_show(args, json_mode),
        TemplateAction::Prune(args) => cmd_prune(args, json_mode),
        TemplateAction::Rm(args) => cmd_rm(args, json_mode),
    }
}

fn cmd_build(args: TemplateBuildArgs, json_mode: bool) -> anyhow::Result<i32> {
    let spec = match load_boot_spec_file(&args.boot_spec) {
        Ok(spec) => spec,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    let (template_store_root, hooks) = match snapshot_restore_parts(&spec, "template build") {
        Ok(parts) => parts,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };

    let request_id = crate::request_id::new();
    let _request_id_scope = crate::request_id::set(request_id.clone());
    let sandbox_config = sandbox_config_for_boot_spec(&spec, request_id);
    let store = match open_or_create_store(&template_store_root) {
        Ok(store) => store,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    let (backend, _effective) = match super::build_backend(&HashMap::new()) {
        Ok(pair) => pair,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    let pinned = match backend.build_snapshot_template(sandbox_config, hooks, &store) {
        Ok(pinned) => pinned,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };

    let data = TemplateBuildOutput {
        name: args.name,
        boot_spec: display_path(&args.boot_spec),
        store: display_path(store.root()),
        fingerprint: pinned.reference().fingerprint().to_hex(),
        manifest: pinned.manifest(),
    };
    render_build_output(&data, json_mode);
    Ok(0)
}

fn cmd_list(args: TemplateStoreArgs, json_mode: bool) -> anyhow::Result<i32> {
    let store = match open_store(&args.store) {
        Ok(store) => store,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    let summaries = match store.list() {
        Ok(summaries) => summaries,
        Err(err) => return Ok(render_store_error(err, json_mode)),
    };
    let data = TemplateListOutput {
        store: display_path(store.root()),
        total_size_bytes: summaries.iter().map(|summary| summary.size_bytes).sum(),
        templates: summaries_json(summaries),
    };
    render_list_output(&data, json_mode);
    Ok(0)
}

fn cmd_show(args: TemplateFingerprintArgs, json_mode: bool) -> anyhow::Result<i32> {
    let (store, fingerprint) = match open_store_and_fingerprint(&args.store, &args.fingerprint) {
        Ok(pair) => pair,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    let manifest = match store.manifest(&fingerprint) {
        Ok(manifest) => manifest,
        Err(err) => return Ok(render_store_error(err, json_mode)),
    };
    let summary = match store.list() {
        Ok(summaries) => summaries
            .into_iter()
            .find(|summary| summary.fingerprint == fingerprint),
        Err(err) => return Ok(render_store_error(err, json_mode)),
    };
    let data = TemplateShowOutput {
        store: display_path(store.root()),
        fingerprint: fingerprint.to_hex(),
        size_bytes: summary.as_ref().map(|summary| summary.size_bytes),
        last_used_unix_ms: summary.as_ref().map(|summary| summary.last_used_unix_ms),
        manifest,
    };
    render_show_output(&data, json_mode);
    Ok(0)
}

fn cmd_prune(args: TemplatePruneArgs, json_mode: bool) -> anyhow::Result<i32> {
    if let Some(boot_spec) = args.boot_spec.as_ref() {
        let spec = match load_boot_spec_file(boot_spec) {
            Ok(spec) => spec,
            Err(err) => return Ok(errors::render_error(&err, json_mode)),
        };
        let (_template_store_root, hooks) = match snapshot_restore_parts(&spec, "template prune") {
            Ok(parts) => parts,
            Err(err) => return Ok(errors::render_error(&err, json_mode)),
        };
        let request_id = crate::request_id::new();
        let _request_id_scope = crate::request_id::set(request_id.clone());
        let sandbox_config = sandbox_config_for_boot_spec(&spec, request_id);
        let (backend, _effective) = match super::build_backend(&HashMap::new()) {
            Ok(pair) => pair,
            Err(err) => return Ok(errors::render_error(&err, json_mode)),
        };
        let live_inputs = match backend.snapshot_template_inputs(&sandbox_config, hooks) {
            Ok(inputs) => inputs,
            Err(err) => return Ok(errors::render_error(&err, json_mode)),
        };
        let store = match open_store(&args.store) {
            Ok(store) => store,
            Err(err) => return Ok(errors::render_error(&err, json_mode)),
        };
        let removed_count = match store.evict_invalidated_in_scope(&live_inputs) {
            Ok(removed_count) => removed_count,
            Err(err) => return Ok(render_store_error(err, json_mode)),
        };
        let data = TemplatePruneOutput {
            store: display_path(store.root()),
            removed_count,
            mode: "scoped_boot_spec",
        };
        render_prune_output(&data, json_mode);
        return Ok(0);
    }

    let store = match open_store(&args.store) {
        Ok(store) => store,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    let data = TemplatePruneOutput {
        store: display_path(store.root()),
        removed_count: 0,
        mode: "noop_without_boot_spec",
    };
    render_prune_output(&data, json_mode);
    Ok(0)
}

fn cmd_rm(args: TemplateFingerprintArgs, json_mode: bool) -> anyhow::Result<i32> {
    let (store, fingerprint) = match open_store_and_fingerprint(&args.store, &args.fingerprint) {
        Ok(pair) => pair,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    if let Err(err) = store.remove(&fingerprint) {
        return Ok(render_store_error(err, json_mode));
    }
    let data = TemplateRemoveOutput {
        store: display_path(store.root()),
        fingerprint: fingerprint.to_hex(),
        removed: true,
    };
    render_remove_output(&data, json_mode);
    Ok(0)
}

fn open_or_create_store(path: &Path) -> Result<TemplateStore, FcError> {
    if path.exists() {
        return Ok(TemplateStore::open(
            path,
            DEFAULT_TEMPLATE_STORE_CAPACITY_FOR_BUILDS,
        )?);
    }
    Ok(TemplateStore::create(
        path,
        DEFAULT_TEMPLATE_STORE_CAPACITY_FOR_BUILDS,
    )?)
}

fn open_store(path: &Path) -> Result<TemplateStore, FcError> {
    Ok(TemplateStore::open(
        path,
        DEFAULT_TEMPLATE_STORE_CAPACITY_FOR_READS,
    )?)
}

fn open_store_and_fingerprint(
    store_path: &Path,
    fingerprint: &str,
) -> Result<(TemplateStore, TemplateFingerprint), FcError> {
    let store = open_store(store_path)?;
    let fingerprint = parse_fingerprint(fingerprint)?;
    Ok((store, fingerprint))
}

fn load_boot_spec_file(path: &Path) -> Result<BootSpec, FcError> {
    let content = std::fs::read_to_string(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => load_boot_spec_json_str(&content),
        Some("yaml" | "yml") => load_boot_spec_yaml_str(&content),
        _ => load_boot_spec_yaml_str(&content).or_else(|yaml_err| {
            load_boot_spec_json_str(&content).map_err(|json_err| {
                FcError::Config(ConfigError::InvalidValue {
                    field: "boot_spec",
                    reason: format!("failed to parse as YAML ({yaml_err}) or JSON ({json_err})"),
                })
            })
        }),
    }
}

fn snapshot_restore_parts(
    spec: &BootSpec,
    operation: &'static str,
) -> Result<(PathBuf, m80_firecracker::HookSpecSet), FcError> {
    match &spec.warm_strategy {
        BootSpecWarmStrategy::SnapshotRestore {
            template_store,
            template_fingerprint: _,
            ready_probe: _,
            hooks,
        } => Ok((template_store.clone(), hooks.clone())),
        BootSpecWarmStrategy::BootFill => Err(FcError::Config(ConfigError::InvalidValue {
            field: "boot_spec.warm_strategy.mode",
            reason: format!("{operation} requires snapshot_restore"),
        })),
    }
}

fn sandbox_config_for_boot_spec(spec: &BootSpec, request_id: String) -> SandboxConfig {
    SandboxConfig {
        vm_id: None,
        workspace: spec.sandbox.workspace.clone(),
        network: spec.sandbox.network.clone(),
        vcpu_count: Some(spec.sandbox.vcpu_count),
        mem_size_mib: Some(spec.sandbox.mem_size_mib),
        huge_pages_2m: false,
        boot_args: (!spec.sandbox.boot_args.is_empty()).then(|| spec.sandbox.boot_args.join(" ")),
        overlay_size_bytes: spec.sandbox.overlay_size_bytes,
        overlay_clone_mode: spec.sandbox.overlay_clone_mode,
        request_id: Some(request_id),
        pmem_layers: spec.pmem_layers.clone(),
        ..SandboxConfig::default()
    }
}

fn parse_fingerprint(value: &str) -> Result<TemplateFingerprint, FcError> {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    TemplateFingerprint::parse_hex(value).map_err(|source| {
        FcError::Config(ConfigError::InvalidValue {
            field: "fingerprint",
            reason: source.to_string(),
        })
    })
}

fn render_store_error(err: m80_snapshot_template::TemplateStoreError, json_mode: bool) -> i32 {
    errors::render_error(&FcError::TemplateStore(err), json_mode)
}

fn render_build_output(data: &TemplateBuildOutput<'_>, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(data));
    } else {
        println!("built template `{}`: {}", data.name, data.fingerprint);
    }
}

fn render_list_output(data: &TemplateListOutput, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(data));
    } else {
        print_summaries(&data.templates);
    }
}

fn render_show_output(data: &TemplateShowOutput, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(data));
    } else {
        println!("fingerprint: {}", data.fingerprint);
        if let Some(size_bytes) = data.size_bytes {
            println!("size_bytes: {size_bytes}");
        }
        if let Some(last_used_unix_ms) = data.last_used_unix_ms {
            println!("last_used_unix_ms: {last_used_unix_ms}");
        }
        println!("schema_version: {}", data.manifest.schema_version);
        println!(
            "pmem_layers: {}",
            data.manifest.inputs.pmem_image_digest_set().len()
        );
        println!(
            "hooks: {}",
            data.manifest.inputs.hook_spec_set().hooks().len()
        );
    }
}

fn render_prune_output(data: &TemplatePruneOutput, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(data));
    } else {
        println!("removed templates: {}", data.removed_count);
    }
}

fn render_remove_output(data: &TemplateRemoveOutput, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(data));
    } else {
        println!("removed template {}", data.fingerprint);
    }
}

fn print_summaries(templates: &[TemplateSummaryJson]) {
    println!("FINGERPRINT\tSIZE_BYTES\tLAST_USED_UNIX_MS");
    for template in templates {
        println!(
            "{}\t{}\t{}",
            template.fingerprint, template.size_bytes, template.last_used_unix_ms
        );
    }
}

fn summaries_json(mut summaries: Vec<TemplateSummary>) -> Vec<TemplateSummaryJson> {
    summaries.sort_by_key(|summary| summary.fingerprint.to_hex());
    summaries
        .into_iter()
        .map(|summary| TemplateSummaryJson {
            fingerprint: summary.fingerprint.to_hex(),
            size_bytes: summary.size_bytes,
            last_used_unix_ms: summary.last_used_unix_ms,
        })
        .collect()
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

#[derive(Serialize)]
struct TemplateBuildOutput<'a> {
    name: String,
    boot_spec: String,
    store: String,
    fingerprint: String,
    manifest: &'a TemplateManifest,
}

#[derive(Serialize)]
struct TemplateListOutput {
    store: String,
    total_size_bytes: u64,
    templates: Vec<TemplateSummaryJson>,
}

#[derive(Serialize)]
struct TemplateShowOutput {
    store: String,
    fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_used_unix_ms: Option<u64>,
    manifest: TemplateManifest,
}

#[derive(Serialize)]
struct TemplatePruneOutput {
    store: String,
    removed_count: usize,
    mode: &'static str,
}

#[derive(Serialize)]
struct TemplateRemoveOutput {
    store: String,
    fingerprint: String,
    removed: bool,
}

#[derive(Serialize)]
struct TemplateSummaryJson {
    fingerprint: String,
    size_bytes: u64,
    last_used_unix_ms: u64,
}
