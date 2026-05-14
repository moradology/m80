//! Build m80 protobuf wire types from the checked-in schema.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const VENDORED_PROTOC_LINUX_X86_64_SHA256: &str =
    "caaf8517e57c57d34a7d6f0544172d9051abf58556aa35c70d3fb0d824b8cfbb";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_path()?;
    std::env::set_var("PROTOC", protoc);

    prost_build::Config::new().compile_protos(&["proto/m80/wire.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/m80/wire.proto");
    println!("cargo:rerun-if-env-changed=PROTOC");
    Ok(())
}

fn protoc_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(protoc) = std::env::var_os("PROTOC") {
        return Ok(PathBuf::from(protoc));
    }

    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    verify_vendored_protoc(&protoc)?;
    Ok(protoc)
}

fn verify_vendored_protoc(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let expected = expected_vendored_protoc_sha256().ok_or_else(|| {
        format!(
            "unsupported vendored protoc target {}; set PROTOC to a system protoc",
            std::env::consts::ARCH
        )
    })?;
    let actual = sha256_file(path)?;
    if actual != expected {
        return Err(format!(
            "vendored protoc sha256 mismatch for {}: expected {expected}, got {actual}",
            path.display()
        )
        .into());
    }
    Ok(())
}

fn expected_vendored_protoc_sha256() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some(VENDORED_PROTOC_LINUX_X86_64_SHA256),
        _ => None,
    }
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}
