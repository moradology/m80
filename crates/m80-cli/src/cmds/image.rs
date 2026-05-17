use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, SystemTime};

use m80_firecracker::{ConfigError, FcError};
use m80_image_store::{ImageDigest, ImageKind, ImageRecord, ImageStore, StoreError};
use m80_snapshot_template::TemplateStore;
use serde::Serialize;

use crate::args::{
    ImageAction, ImageBuildArgs, ImageDigestArgs, ImageGcArgs, ImageRmArgs, ImageStoreArgs,
};
use crate::{errors, json};

const DEFAULT_TEMPLATE_STORE_CAPACITY_FOR_REFERENCE_CHECK: usize = usize::MAX;

pub(super) fn cmd_image(action: ImageAction, json_mode: bool) -> anyhow::Result<i32> {
    match action {
        ImageAction::Build(args) => cmd_build(args, json_mode),
        ImageAction::Gc(args) => cmd_gc(args, json_mode),
        ImageAction::List(args) => cmd_list(args, json_mode),
        ImageAction::Show(args) => cmd_show(args, json_mode),
        ImageAction::Rm(args) => cmd_rm(args, json_mode),
        ImageAction::Verify(args) => cmd_verify(args, json_mode),
    }
}

fn cmd_build(args: ImageBuildArgs, json_mode: bool) -> anyhow::Result<i32> {
    let store = match open_or_create_store(&args.out) {
        Ok(store) => store,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    let kind: ImageKind = args.kind.into();
    let digest = if args.source.is_dir() {
        store.build_minimal_test_image(&args.source, kind)
    } else {
        store.import_existing(&args.source, kind)
    };
    let digest = match digest {
        Ok(digest) => digest,
        Err(err) => return Ok(render_store_error(err, json_mode)),
    };
    let records = match store.describe(&digest) {
        Ok(records) => records,
        Err(err) => return Ok(render_store_error(err, json_mode)),
    };
    let data = ImageBuildOutput {
        name: args.name,
        source: display_path(&args.source),
        store: display_path(store.root()),
        digest: digest.as_str().to_owned(),
        images: records_json(&records),
    };
    render_build_output(&data, json_mode);
    Ok(0)
}

fn cmd_gc(args: ImageGcArgs, json_mode: bool) -> anyhow::Result<i32> {
    let store = match ImageStore::open(&args.store) {
        Ok(store) => store,
        Err(err) => return Ok(render_store_error(err, json_mode)),
    };
    let pins = match gc_pins(&args.keep, args.pin_file.as_deref()) {
        Ok(pins) => pins,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    let min_age = match args.min_age.as_deref().map(parse_duration) {
        Some(Ok(duration)) => Some(duration),
        Some(Err(err)) => return Ok(errors::render_error(&err, json_mode)),
        None => None,
    };

    let mut data = if args.execute {
        let _guard = match store.acquire_gc_execute_guard() {
            Ok(guard) => guard,
            Err(err) => return Ok(render_store_error(err, json_mode)),
        };
        let mut data = match gc_plan(&store, &args, &pins, min_age) {
            Ok(data) => data,
            Err(err) => return Ok(errors::render_error(&err, json_mode)),
        };
        if let Err(err) = execute_gc_plan(&store, &args.template_store, &mut data) {
            return Ok(errors::render_error(&err, json_mode));
        }
        data
    } else {
        match gc_plan(&store, &args, &pins, min_age) {
            Ok(data) => data,
            Err(err) => return Ok(errors::render_error(&err, json_mode)),
        }
    };
    data.execute = args.execute;
    render_gc_output(&data, json_mode);
    Ok(0)
}

fn cmd_list(args: ImageStoreArgs, json_mode: bool) -> anyhow::Result<i32> {
    let store = match ImageStore::open(&args.store) {
        Ok(store) => store,
        Err(err) => return Ok(render_store_error(err, json_mode)),
    };
    let records = match store.list() {
        Ok(records) => records,
        Err(err) => return Ok(render_store_error(err, json_mode)),
    };
    let data = ImageListOutput {
        store: display_path(store.root()),
        total_size_bytes: records.iter().map(ImageRecord::size_bytes).sum(),
        images: records_json(&records),
    };
    render_list_output(&data, json_mode);
    Ok(0)
}

fn cmd_show(args: ImageDigestArgs, json_mode: bool) -> anyhow::Result<i32> {
    let (store, digest) = match open_store_and_digest(&args.store, &args.digest) {
        Ok(pair) => pair,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    let records = match store.describe(&digest) {
        Ok(records) => records,
        Err(err) => return Ok(render_store_error(err, json_mode)),
    };
    let data = ImageShowOutput {
        store: display_path(store.root()),
        digest: digest.as_str().to_owned(),
        images: records_json(&records),
    };
    render_show_output(&data, json_mode);
    Ok(0)
}

fn cmd_rm(args: ImageRmArgs, json_mode: bool) -> anyhow::Result<i32> {
    let (store, digest) = match open_store_and_digest(&args.store, &args.digest) {
        Ok(pair) => pair,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    let references = match template_references(&args.template_store, &digest) {
        Ok(references) => references,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    if !references.is_empty() {
        return Ok(render_store_error(
            StoreError::ImageReferencedByTemplate {
                digest,
                template_fingerprints: references,
            },
            json_mode,
        ));
    }
    let removed = match store.remove(&digest) {
        Ok(removed) => removed,
        Err(err) => return Ok(render_store_error(err, json_mode)),
    };
    let data = ImageRemoveOutput {
        store: display_path(store.root()),
        digest: digest.as_str().to_owned(),
        removed: records_json(&removed),
    };
    render_remove_output(&data, json_mode);
    Ok(0)
}

fn cmd_verify(args: ImageDigestArgs, json_mode: bool) -> anyhow::Result<i32> {
    let (store, digest) = match open_store_and_digest(&args.store, &args.digest) {
        Ok(pair) => pair,
        Err(err) => return Ok(errors::render_error(&err, json_mode)),
    };
    if let Err(err) = store.verify(&digest) {
        return Ok(render_store_error(err, json_mode));
    }
    let records = match store.describe(&digest) {
        Ok(records) => records,
        Err(err) => return Ok(render_store_error(err, json_mode)),
    };
    let data = ImageVerifyOutput {
        store: display_path(store.root()),
        digest: digest.as_str().to_owned(),
        verified: true,
        images: records_json(&records),
    };
    render_verify_output(&data, json_mode);
    Ok(0)
}

fn open_or_create_store(path: &Path) -> Result<ImageStore, FcError> {
    if path.exists() {
        return Ok(ImageStore::open(path)?);
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let Some(parent) = parent else {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "out",
            reason: "image-store path must have a parent directory".into(),
        }));
    };
    if !parent.exists() {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "out",
            reason: "image-store parent directory must already exist".into(),
        }));
    }
    std::fs::create_dir(path).map_err(|source| FcError::PathIo {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(ImageStore::open(path)?)
}

fn open_store_and_digest(
    store_path: &Path,
    digest: &str,
) -> Result<(ImageStore, ImageDigest), FcError> {
    let store = ImageStore::open(store_path)?;
    let digest = parse_digest(digest)?;
    Ok((store, digest))
}

fn parse_digest(value: &str) -> Result<ImageDigest, FcError> {
    parse_digest_for_field(value, "digest")
}

fn parse_digest_for_field(value: &str, field: &'static str) -> Result<ImageDigest, FcError> {
    ImageDigest::parse(value).map_err(|source| {
        FcError::Config(ConfigError::InvalidValue {
            field,
            reason: source.to_string(),
        })
    })
}

fn template_references(
    template_store_root: &Path,
    digest: &ImageDigest,
) -> Result<Vec<String>, FcError> {
    if !template_store_root.exists() {
        return Ok(Vec::new());
    }
    let store = TemplateStore::open(
        template_store_root,
        DEFAULT_TEMPLATE_STORE_CAPACITY_FOR_REFERENCE_CHECK,
    )?;
    let template_digest = m80_snapshot_template::ImageDigest::parse(digest.as_str())
        .map_err(FcError::TemplateStore)?;
    Ok(store
        .templates_referencing_image(&template_digest)?
        .into_iter()
        .map(|fingerprint| fingerprint.to_hex())
        .collect())
}

fn render_store_error(err: StoreError, json_mode: bool) -> i32 {
    errors::render_error(&FcError::ImageStore(err), json_mode)
}

fn render_build_output(data: &ImageBuildOutput, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(data));
    } else {
        println!("built image `{}`: {}", data.name, data.digest);
        print_records(&data.images);
    }
}

fn render_list_output(data: &ImageListOutput, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(data));
    } else {
        print_records(&data.images);
    }
}

fn render_show_output(data: &ImageShowOutput, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(data));
    } else {
        print_records(&data.images);
    }
}

fn render_remove_output(data: &ImageRemoveOutput, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(data));
    } else {
        println!("removed image {}", data.digest);
        print_records(&data.removed);
    }
}

fn render_verify_output(data: &ImageVerifyOutput, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(data));
    } else {
        println!("verified image {}", data.digest);
        print_records(&data.images);
    }
}

fn render_gc_output(data: &ImageGcOutput, json_mode: bool) {
    if json_mode {
        println!("{}", json::to_pretty(data));
    } else {
        println!("STATUS\tDIGEST\tSIZE_BYTES\tREASONS");
        for image in &data.images {
            let reasons = if image.reasons.is_empty() {
                "-".to_owned()
            } else {
                image.reasons.join(",")
            };
            println!(
                "{}\t{}\t{}\t{}",
                image.status, image.digest, image.size_bytes, reasons
            );
        }
        println!("total_reclaimable_bytes={}", data.total_reclaimable_bytes);
        if data.execute {
            println!("total_removed_bytes={}", data.total_removed_bytes);
        } else {
            println!("dry_run=true");
        }
    }
}

fn print_records(records: &[ImageRecordJson]) {
    println!("KIND\tDIGEST\tSIZE_BYTES\tPATH");
    for record in records {
        println!(
            "{}\t{}\t{}\t{}",
            record.kind, record.digest, record.size_bytes, record.path
        );
    }
}

fn records_json(records: &[ImageRecord]) -> Vec<ImageRecordJson> {
    records
        .iter()
        .map(|record| ImageRecordJson {
            digest: record.digest().as_str().to_owned(),
            kind: record.kind().as_str(),
            size_bytes: record.size_bytes(),
            path: display_path(record.path()),
        })
        .collect()
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

fn gc_pins(keep: &[String], pin_file: Option<&Path>) -> Result<ImageGcPins, FcError> {
    let mut pins = ImageGcPins::default();
    for digest in keep {
        pins.keep
            .insert(parse_digest_for_field(digest, "keep")?.as_str().to_owned());
    }
    if let Some(path) = pin_file {
        let contents = std::fs::read_to_string(path).map_err(|source| FcError::PathIo {
            path: path.to_path_buf(),
            source,
        })?;
        for (line_index, line) in contents.lines().enumerate() {
            let digest = line.trim();
            if digest.is_empty() {
                continue;
            }
            let parsed = ImageDigest::parse(digest).map_err(|source| {
                FcError::Config(ConfigError::InvalidValue {
                    field: "pin-file",
                    reason: format!("line {}: {}", line_index + 1, source),
                })
            })?;
            pins.pin_file.insert(parsed.as_str().to_owned());
        }
    }
    Ok(pins)
}

fn parse_duration(value: &str) -> Result<Duration, FcError> {
    let Some(split_at) = value.find(|ch: char| !ch.is_ascii_digit()) else {
        return duration_from_parts(value, "", value);
    };
    let (amount, unit) = value.split_at(split_at);
    duration_from_parts(amount, unit, value)
}

fn duration_from_parts(amount: &str, unit: &str, original: &str) -> Result<Duration, FcError> {
    if amount.is_empty() {
        return Err(invalid_config(
            "min-age",
            format!("duration `{original}` must start with digits"),
        ));
    }
    let amount = amount.parse::<u64>().map_err(|_| {
        invalid_config(
            "min-age",
            format!("duration `{original}` must fit in u64 seconds"),
        )
    })?;
    let multiplier = match unit {
        "" | "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => {
            return Err(invalid_config(
                "min-age",
                format!("duration `{original}` must use suffix s, m, h, or d"),
            ));
        }
    };
    let seconds = amount
        .checked_mul(multiplier)
        .ok_or_else(|| invalid_config("min-age", format!("duration `{original}` is too large")))?;
    Ok(Duration::from_secs(seconds))
}

fn invalid_config(field: &'static str, reason: String) -> FcError {
    FcError::Config(ConfigError::InvalidValue { field, reason })
}

fn gc_plan(
    store: &ImageStore,
    args: &ImageGcArgs,
    pins: &ImageGcPins,
    min_age: Option<Duration>,
) -> Result<ImageGcOutput, FcError> {
    let cutoff = match min_age {
        Some(duration) => Some(SystemTime::now().checked_sub(duration).ok_or_else(|| {
            invalid_config(
                "min-age",
                "duration is too large for the current system clock".to_owned(),
            )
        })?),
        None => None,
    };
    let records = store.list()?;
    let mut by_digest = BTreeMap::<String, Vec<ImageRecord>>::new();
    for record in records {
        by_digest
            .entry(record.digest().as_str().to_owned())
            .or_default()
            .push(record);
    }

    let mut images = Vec::with_capacity(by_digest.len());
    let mut total_reclaimable_bytes = 0;
    for (digest_str, records) in by_digest {
        let digest = parse_digest(&digest_str)?;
        let mut reasons = Vec::new();
        if pins.keep.contains(digest.as_str()) {
            reasons.push("keep".to_owned());
        }
        if pins.pin_file.contains(digest.as_str()) {
            reasons.push("pin-file".to_owned());
        }
        let shared_ref_count = store.shared_ref_count(&digest)?;
        if shared_ref_count > 0 {
            reasons.push(format!("shared-ref:{shared_ref_count}"));
        }
        let template_refs = template_references(&args.template_store, &digest)?;
        if !template_refs.is_empty() {
            reasons.push(format!("template:{}", template_refs.join(",")));
        }
        if let Some(cutoff) = cutoff {
            let newest = newest_record_modified_time(&records)?;
            if newest > cutoff {
                let min_age = args.min_age.as_deref().unwrap_or_default();
                reasons.push(format!("min-age:{min_age}"));
            }
        }
        let size_bytes = records.iter().map(ImageRecord::size_bytes).sum();
        let status = if reasons.is_empty() {
            total_reclaimable_bytes += size_bytes;
            "candidate"
        } else {
            "protected"
        };
        images.push(ImageGcImageOutput {
            digest: digest.as_str().to_owned(),
            status,
            size_bytes,
            reasons,
            images: records_json(&records),
            removed: Vec::new(),
        });
    }

    Ok(ImageGcOutput {
        store: display_path(store.root()),
        template_store: display_path(&args.template_store),
        execute: args.execute,
        dry_run: !args.execute,
        total_reclaimable_bytes,
        total_removed_bytes: 0,
        images,
    })
}

fn newest_record_modified_time(records: &[ImageRecord]) -> Result<SystemTime, FcError> {
    let mut newest = None;
    for record in records {
        let modified = record
            .path()
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|source| FcError::PathIo {
                path: record.path().to_path_buf(),
                source,
            })?;
        if newest.as_ref().is_none_or(|current| modified > *current) {
            newest = Some(modified);
        }
    }
    newest.ok_or_else(|| {
        invalid_config(
            "image-store",
            "gc candidate must contain at least one image record".to_owned(),
        )
    })
}

fn execute_gc_plan(
    store: &ImageStore,
    template_store: &Path,
    data: &mut ImageGcOutput,
) -> Result<(), FcError> {
    let mut total_removed_bytes = 0;
    for image in &mut data.images {
        if image.status != "candidate" {
            continue;
        }
        let digest = parse_digest(&image.digest)?;
        let references = template_references(template_store, &digest)?;
        if !references.is_empty() {
            return Err(StoreError::ImageReferencedByTemplate {
                digest,
                template_fingerprints: references,
            }
            .into());
        }
        let removed = store.remove(&digest)?;
        total_removed_bytes += removed.iter().map(ImageRecord::size_bytes).sum::<u64>();
        image.status = "removed";
        image.removed = records_json(&removed);
    }
    data.total_removed_bytes = total_removed_bytes;
    data.dry_run = false;
    Ok(())
}

#[derive(Debug, Default)]
struct ImageGcPins {
    keep: BTreeSet<String>,
    pin_file: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
struct ImageRecordJson {
    digest: String,
    kind: &'static str,
    size_bytes: u64,
    path: String,
}

#[derive(Debug, Serialize)]
struct ImageBuildOutput {
    name: String,
    source: String,
    store: String,
    digest: String,
    images: Vec<ImageRecordJson>,
}

#[derive(Debug, Serialize)]
struct ImageListOutput {
    store: String,
    total_size_bytes: u64,
    images: Vec<ImageRecordJson>,
}

#[derive(Debug, Serialize)]
struct ImageGcOutput {
    store: String,
    template_store: String,
    execute: bool,
    dry_run: bool,
    total_reclaimable_bytes: u64,
    total_removed_bytes: u64,
    images: Vec<ImageGcImageOutput>,
}

#[derive(Debug, Serialize)]
struct ImageGcImageOutput {
    digest: String,
    status: &'static str,
    size_bytes: u64,
    reasons: Vec<String>,
    images: Vec<ImageRecordJson>,
    removed: Vec<ImageRecordJson>,
}

#[derive(Debug, Serialize)]
struct ImageShowOutput {
    store: String,
    digest: String,
    images: Vec<ImageRecordJson>,
}

#[derive(Debug, Serialize)]
struct ImageRemoveOutput {
    store: String,
    digest: String,
    removed: Vec<ImageRecordJson>,
}

#[derive(Debug, Serialize)]
struct ImageVerifyOutput {
    store: String,
    digest: String,
    verified: bool,
    images: Vec<ImageRecordJson>,
}
