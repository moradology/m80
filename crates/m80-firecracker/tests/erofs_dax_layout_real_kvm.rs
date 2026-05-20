//! Real-KVM layout gate for erofs+DAX Shared pmem density claims.
//!
//! This is owned by `m80-q420k.8.12`. It proves the file-layout distinction
//! that Phase C/F density measurements rely on: `dax=always` on the mount is
//! necessary but not sufficient. The file itself must be an uncompressed,
//! non-inlined erofs inode to receive `STATX_ATTR_DAX`.

use std::fmt::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use m80_image_store::ImageKind;

mod common;
mod pmem_shared_support;

use pmem_shared_support as support;

#[test]
#[ignore = "requires KVM host, real Firecracker binary, stripped erofs+DAX kernel, and root privileges"]
fn erofs_dax_layout_matrix_real_kvm() {
    let _serial = support::REAL_KVM_LOCK.lock().expect("real-kvm test lock");

    let work = tempfile::tempdir().expect("erofs dax layout workdir");
    let probe = compile_statx_probe(work.path());
    let store = support::open_default_store();
    let backend = support::real_backend(1);
    let cases = [
        LayoutCase::new(
            "flat-uncompressed",
            ErofsCompression::Uncompressed,
            SourceShape::FlatPayload,
            true,
            "mkfs.erofs without -z",
        ),
        LayoutCase::new(
            "flat-compressed-lz4hc",
            ErofsCompression::Lz4hc,
            SourceShape::FlatPayload,
            false,
            "mkfs.erofs -zlz4hc,level=9",
        ),
        LayoutCase::new(
            "toolchain-proxy-compressed-lz4hc",
            ErofsCompression::Lz4hc,
            SourceShape::ToolchainProxy,
            false,
            "mkfs.erofs -zlz4hc,level=9 with bin/lib/include tree",
        ),
    ];

    let mut results = Vec::new();
    for case in cases {
        let source = work.path().join(format!("src-{}", case.name));
        std::fs::create_dir(&source).expect("create case source");
        write_case_source(&source, &probe, case.shape);
        let image = work.path().join(format!("{}.erofs", case.name));
        build_erofs(&source, &image, case.compression);
        let dump = dump_erofs_payload(&image, case.payload_path());
        let digest = store
            .import_existing(&image, ImageKind::Erofs)
            .expect("import erofs layout case");
        let image_bytes = std::fs::metadata(&image)
            .expect("case image metadata")
            .len();
        let mut running = support::launch_with_layers(
            &backend.backend,
            case.vm_prefix(),
            vec![support::shared_layer(&digest, 0)],
        );
        let mount_line = support::assert_one_pmem_mount(&mut running, 0);
        let guest_probe_path = format!("/opt/m80-layers/smoke-0/{}", case.probe_path());
        let guest_payload_path = format!("/opt/m80-layers/smoke-0/{}", case.payload_path());
        let output = support::exec_stdout(
            &mut running,
            &format!(
                "{guest_probe_path} {guest_payload_path}; dd if={guest_payload_path} of=/dev/null bs=4M status=none"
            ),
        );
        support::stop_and_delete(running);

        let statx = StatxProbe::parse(&output);
        assert_eq!(
            statx.dax, case.expect_dax,
            "case {} expected dax={} but got output {output:?}; dump:\n{dump}",
            case.name, case.expect_dax
        );
        if matches!(case.compression, ErofsCompression::Lz4hc) {
            assert!(
                dump.contains("Layout: 3"),
                "compressed case should use compact compressed layout; dump:\n{dump}"
            );
        } else {
            assert!(
                dump.contains("Layout: 0"),
                "uncompressed case should use plain layout; dump:\n{dump}"
            );
        }

        results.push(CaseResult {
            name: case.name,
            mkfs_shape: case.mkfs_shape,
            image_bytes,
            dump,
            mount_line,
            statx,
        });
    }

    assert!(
        results.iter().any(|result| result.statx.dax)
            && results.iter().any(|result| !result.statx.dax),
        "layout matrix must prove both DAX and non-DAX erofs files"
    );

    if let Some(path) = std::env::var_os("M80_EROFS_DAX_LAYOUT_ARTIFACT") {
        write_artifact(&PathBuf::from(path), &results);
    }
}

#[derive(Clone, Copy)]
struct LayoutCase {
    name: &'static str,
    compression: ErofsCompression,
    shape: SourceShape,
    expect_dax: bool,
    mkfs_shape: &'static str,
}

impl LayoutCase {
    fn new(
        name: &'static str,
        compression: ErofsCompression,
        shape: SourceShape,
        expect_dax: bool,
        mkfs_shape: &'static str,
    ) -> Self {
        Self {
            name,
            compression,
            shape,
            expect_dax,
            mkfs_shape,
        }
    }

    fn payload_path(self) -> &'static str {
        match self.shape {
            SourceShape::FlatPayload => "payload.bin",
            SourceShape::ToolchainProxy => "lib/libpayload.a",
        }
    }

    fn probe_path(self) -> &'static str {
        match self.shape {
            SourceShape::FlatPayload => "statx_dax_probe",
            SourceShape::ToolchainProxy => "bin/statx_dax_probe",
        }
    }

    fn vm_prefix(self) -> &'static str {
        match self.name {
            "flat-uncompressed" => "edl-u",
            "flat-compressed-lz4hc" => "edl-c",
            "toolchain-proxy-compressed-lz4hc" => "edl-t",
            _ => "edl",
        }
    }
}

#[derive(Clone, Copy)]
enum ErofsCompression {
    Uncompressed,
    Lz4hc,
}

#[derive(Clone, Copy)]
enum SourceShape {
    FlatPayload,
    ToolchainProxy,
}

struct CaseResult {
    name: &'static str,
    mkfs_shape: &'static str,
    image_bytes: u64,
    dump: String,
    mount_line: String,
    statx: StatxProbe,
}

struct StatxProbe {
    mask_hex: String,
    attrs_hex: String,
    size: u64,
    dax: bool,
    compressed: bool,
}

impl StatxProbe {
    fn parse(raw: &str) -> Self {
        let mut mask_hex = None;
        let mut attrs_hex = None;
        let mut size = None;
        let mut dax = None;
        let mut compressed = None;
        for field in raw.split_whitespace() {
            if let Some(value) = field.strip_prefix("mask=") {
                mask_hex = Some(value.to_owned());
            } else if let Some(value) = field.strip_prefix("attrs=") {
                attrs_hex = Some(value.to_owned());
            } else if let Some(value) = field.strip_prefix("size=") {
                size = Some(value.parse::<u64>().expect("statx size is integer"));
            } else if let Some(value) = field.strip_prefix("dax=") {
                dax = Some(parse_probe_bool("dax", value));
            } else if let Some(value) = field.strip_prefix("compressed=") {
                compressed = Some(parse_probe_bool("compressed", value));
            }
        }
        Self {
            mask_hex: mask_hex.unwrap_or_else(|| panic!("missing mask in probe output: {raw:?}")),
            attrs_hex: attrs_hex
                .unwrap_or_else(|| panic!("missing attrs in probe output: {raw:?}")),
            size: size.unwrap_or_else(|| panic!("missing size in probe output: {raw:?}")),
            dax: dax.unwrap_or_else(|| panic!("missing dax in probe output: {raw:?}")),
            compressed: compressed
                .unwrap_or_else(|| panic!("missing compressed in probe output: {raw:?}")),
        }
    }
}

fn parse_probe_bool(name: &str, value: &str) -> bool {
    match value {
        "0" => false,
        "1" => true,
        other => panic!("{name} must be 0 or 1, got {other:?}"),
    }
}

fn compile_statx_probe(work: &Path) -> PathBuf {
    let source = work.join("statx_dax_probe.c");
    let output = work.join("statx_dax_probe");
    std::fs::write(&source, STATX_DAX_PROBE_C).expect("write statx probe source");
    let static_status = Command::new("cc")
        .arg("-O2")
        .arg("-static")
        .arg("-o")
        .arg(&output)
        .arg(&source)
        .status()
        .expect("spawn cc for static statx probe");
    if !static_status.success() {
        let dynamic_status = Command::new("cc")
            .arg("-O2")
            .arg("-o")
            .arg(&output)
            .arg(&source)
            .status()
            .expect("spawn cc for dynamic statx probe");
        assert!(dynamic_status.success(), "cc failed for statx probe");
    }
    let mut permissions = std::fs::metadata(&output)
        .expect("statx probe metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&output, permissions).expect("chmod statx probe");
    output
}

fn write_case_source(source: &Path, probe: &Path, shape: SourceShape) {
    match shape {
        SourceShape::FlatPayload => {
            std::fs::write(source.join("payload.bin"), vec![0x5a; 32 * 1024 * 1024])
                .expect("write flat payload");
            std::fs::copy(probe, source.join("statx_dax_probe")).expect("copy flat probe");
            chmod_executable(&source.join("statx_dax_probe"));
        }
        SourceShape::ToolchainProxy => {
            std::fs::create_dir(source.join("bin")).expect("create proxy bin");
            std::fs::create_dir(source.join("lib")).expect("create proxy lib");
            std::fs::create_dir(source.join("include")).expect("create proxy include");
            std::fs::copy(probe, source.join("bin/statx_dax_probe")).expect("copy proxy probe");
            chmod_executable(&source.join("bin/statx_dax_probe"));
            std::fs::write(
                source.join("bin/tool"),
                b"#!/bin/sh\nprintf 'toolchain proxy\\n'\n".as_slice(),
            )
            .expect("write proxy tool");
            chmod_executable(&source.join("bin/tool"));
            std::fs::write(source.join("include/payload.h"), b"#define PAYLOAD 1\n")
                .expect("write proxy header");
            std::fs::write(
                source.join("lib/libpayload.a"),
                vec![0x33; 32 * 1024 * 1024],
            )
            .expect("write proxy archive payload");
        }
    }
}

fn chmod_executable(path: &Path) {
    let mut permissions = std::fs::metadata(path)
        .unwrap_or_else(|err| panic!("metadata {}: {err}", path.display()))
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)
        .unwrap_or_else(|err| panic!("chmod {}: {err}", path.display()));
}

fn build_erofs(source: &Path, image: &Path, compression: ErofsCompression) {
    let mut command = Command::new("mkfs.erofs");
    command
        .arg("--quiet")
        .arg("-T")
        .arg("0")
        .arg("--all-root")
        .arg("--force-uid=0")
        .arg("--force-gid=0")
        .arg("-U")
        .arg("00000000-0000-0000-0000-000000000000")
        .arg("--sort=path");
    if matches!(compression, ErofsCompression::Lz4hc) {
        command.arg("-zlz4hc,level=9");
    }
    let status = command
        .arg(image)
        .arg(source)
        .status()
        .expect("spawn mkfs.erofs");
    assert!(
        status.success(),
        "mkfs.erofs failed for {}",
        image.display()
    );
}

fn dump_erofs_payload(image: &Path, payload_path: &str) -> String {
    let output = Command::new("dump.erofs")
        .arg(format!("--path=/{payload_path}"))
        .arg(image)
        .output()
        .expect("spawn dump.erofs");
    assert!(
        output.status.success(),
        "dump.erofs failed for {}: {}",
        image.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("dump.erofs output is utf8")
}

fn write_artifact(path: &Path, results: &[CaseResult]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create erofs layout artifact parent");
    }
    let mut out = String::new();
    writeln!(out, "# Erofs DAX sharing layout\n").unwrap();
    writeln!(out, "Bead: `m80-q420k.8.12`\n").unwrap();
    writeln!(out, "## Result\n").unwrap();
    writeln!(
        out,
        "Only uncompressed non-inlined erofs files are eligible for file-level DAX on this substrate. Compressed erofs files still mount on the pmem device with `dax=always`, but `statx` inside the guest does not report `STATX_ATTR_DAX` for the file."
    )
    .unwrap();
    writeln!(
        out,
        "Therefore Phase C/F Shared-pmem density measurements must use an uncompressed erofs payload layout, or a later typed import/build validator must reject compressed Shared-layer inputs before the measurement path.\n"
    )
    .unwrap();
    writeln!(out, "## Reproduction\n").unwrap();
    let repro = std::env::var("M80_EROFS_DAX_LAYOUT_REPRO_COMMAND").unwrap_or_else(|_| {
        format!(
            "sudo -n env M80_EROFS_DAX_LAYOUT_ARTIFACT={} cargo test -p m80-firecracker --test erofs_dax_layout_real_kvm -- --ignored --exact erofs_dax_layout_matrix_real_kvm --nocapture",
            path.display()
        )
    });
    writeln!(out, "Command: `{}`\n", repro).unwrap();
    writeln!(out, "## Substrate\n").unwrap();
    writeln!(out, "- host kernel: `{}`", command_output("uname", &["-r"])).unwrap();
    writeln!(
        out,
        "- firecracker: `{}`",
        command_output("/opt/firecracker/bin/firecracker", &["--version"])
    )
    .unwrap();
    writeln!(
        out,
        "- erofs-utils: `{}`",
        command_output("mkfs.erofs", &["-V"])
    )
    .unwrap();
    writeln!(
        out,
        "- kernel/userland rule: local `erofs(5)` documents that `dax=always` direct reads apply to uncompressed non-inlined files.\n"
    )
    .unwrap();
    writeln!(out, "## Matrix\n").unwrap();
    writeln!(
        out,
        "| case | mkfs shape | image bytes | statx size | dump layout | statx dax | statx compressed | statx mask | statx attrs | mount |"
    )
    .unwrap();
    writeln!(out, "|---|---|---:|---:|---|---:|---:|---|---|---|").unwrap();
    for result in results {
        let layout = result
            .dump
            .lines()
            .find_map(|line| line.trim().strip_prefix("NID:"))
            .unwrap_or("missing layout");
        writeln!(
            out,
            "| `{}` | `{}` | {} | {} | `{}` | {} | {} | `{}` | `{}` | `{}` |",
            result.name,
            result.mkfs_shape,
            result.image_bytes,
            result.statx.size,
            layout.replace('|', "\\|"),
            result.statx.dax,
            result.statx.compressed,
            result.statx.mask_hex,
            result.statx.attrs_hex,
            result.mount_line.replace('|', "\\|")
        )
        .unwrap();
    }
    writeln!(out, "\n## Raw dump.erofs output\n").unwrap();
    for result in results {
        writeln!(out, "### {}\n", result.name).unwrap();
        writeln!(out, "```text\n{}```\n", result.dump).unwrap();
    }
    std::fs::write(path, out).expect("write erofs layout artifact");
    println!("M80_EROFS_DAX_LAYOUT_ARTIFACT {}", path.display());
}

fn command_output(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("spawn {program}: {err}"));
    let raw = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    String::from_utf8_lossy(raw)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.contains("Firecracker exiting successfully"))
        .collect::<Vec<_>>()
        .join("; ")
}

const STATX_DAX_PROBE_C: &str = r#"
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/stat.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#ifndef STATX_ATTR_DAX
#define STATX_ATTR_DAX 0x00200000U
#endif

#ifndef STATX_ATTR_COMPRESSED
#define STATX_ATTR_COMPRESSED 0x00000004U
#endif

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s PATH\n", argv[0]);
        return 2;
    }

    struct statx stx;
    memset(&stx, 0, sizeof(stx));
    if (statx(AT_FDCWD, argv[1], AT_SYMLINK_NOFOLLOW, STATX_ALL, &stx) != 0) {
        fprintf(stderr, "statx(%s): %s\n", argv[1], strerror(errno));
        return 1;
    }

    printf("path=%s mask=0x%x attrs=0x%llx size=%llu dax=%u compressed=%u\n",
           argv[1],
           stx.stx_mask,
           (unsigned long long)stx.stx_attributes,
           (unsigned long long)stx.stx_size,
           (stx.stx_attributes & STATX_ATTR_DAX) ? 1U : 0U,
           (stx.stx_attributes & STATX_ATTR_COMPRESSED) ? 1U : 0U);
    return 0;
}
"#;
