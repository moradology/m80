use super::support::*;

#[test]
fn quickstart_no_run_installs_verified_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    let paths = install_paths(&dir, "default");

    m80()
        .args(quickstart_no_run_args(&tarball, &paths))
        .assert()
        .success();

    for artifact in [
        "vmlinux",
        "output.ext4",
        "output.ext4.manifest.json",
        "m80-guestd",
    ] {
        assert!(
            paths.dst.join(artifact).is_file(),
            "quickstart should install {artifact}"
        );
    }
    assert!(paths.run_root.is_dir(), "quickstart should create run-root");

    let manifest = Manifest::read(&paths.dst.join("output.ext4.manifest.json")).unwrap();
    assert_eq!(manifest.kernel_image, paths.dst.join("vmlinux"));
    assert_eq!(manifest.output_rootfs_image, paths.dst.join("output.ext4"));
    assert_eq!(manifest.daemon_binary_path, paths.dst.join("m80-guestd"));
    manifest.verify(&paths.dst).unwrap();
    let receipt = BuildReceipt::read(&paths.dst.join("output.ext4.build-receipt.json")).unwrap();
    assert_eq!(
        receipt.manifest_path,
        paths.dst.join("output.ext4.manifest.json")
    );
    assert_eq!(
        receipt.manifest_sha256,
        sha256_hex(&paths.dst.join("output.ext4.manifest.json"))
    );
    let provenance = InstallProvenance::read(&paths.dst.join("install-provenance.json")).unwrap();
    assert_eq!(provenance.release_tag, None);
    assert_eq!(provenance.transforms.len(), 2);
    assert_rewrite_record(
        &provenance,
        InstallProvenanceArtifact::GuestManifest,
        "output.ext4.manifest.json",
        &paths.dst.join("output.ext4.manifest.json"),
    );
    assert_rewrite_record(
        &provenance,
        InstallProvenanceArtifact::BuildReceipt,
        "output.ext4.build-receipt.json",
        &paths.dst.join("output.ext4.build-receipt.json"),
    );
    assert!(
        !paths.dst.join("host-binaries.manifest.json").exists(),
        "quickstart must not install a bundled host-binaries manifest"
    );

    let profile_path = paths.profile_dir.join("default.toml");
    let profile = read_toml(&profile_path);
    assert_eq!(
        toml_str(&profile, "artifact_dir"),
        paths.dst.to_str().unwrap()
    );
    assert_eq!(
        toml_str(&profile, "kernel_image"),
        paths.dst.join("vmlinux").to_str().unwrap()
    );
    assert_eq!(
        toml_str(&profile, "rootfs_image"),
        paths.dst.join("output.ext4").to_str().unwrap()
    );
    assert_eq!(toml_str(&profile, "kernel_kind"), "stock");
    assert_eq!(
        toml_str(&profile, "guestd"),
        paths.dst.join("m80-guestd").to_str().unwrap()
    );
    assert_eq!(
        toml_str(&profile, "guest_manifest"),
        paths
            .dst
            .join("output.ext4.manifest.json")
            .to_str()
            .unwrap()
    );
    assert_eq!(
        toml_str(&profile, "build_receipt"),
        paths
            .dst
            .join("output.ext4.build-receipt.json")
            .to_str()
            .unwrap()
    );
    assert_eq!(
        toml_str(&profile, "install_provenance"),
        paths.dst.join("install-provenance.json").to_str().unwrap()
    );
    assert_eq!(
        toml_str(&profile, "host_binaries_manifest"),
        paths
            .dst
            .join("host-binaries.manifest.json")
            .to_str()
            .unwrap()
    );
    assert_eq!(
        toml_str(&profile, "run_root"),
        paths.run_root.to_str().unwrap()
    );
    assert_eq!(
        toml_str(&profile, "m80_version"),
        concat!(env!("CARGO_PKG_VERSION"), "-dev")
    );
    assert!(profile.get("release_tag").is_none());

    let config = read_toml(&paths.config_path);
    assert_eq!(toml_str(&config, "default_profile"), "default");
    assert_eq!(
        toml_str(&config, "run_root"),
        paths.run_root.to_str().unwrap()
    );
}

#[test]
fn quickstart_rejects_tarball_when_external_checksum_mismatches() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = write_release_tarball(&dir);
    std::fs::write(
        format!("{}.sha256", tarball.display()),
        "0000000000000000000000000000000000000000000000000000000000000000  m80-artifacts.tar.gz\n",
    )
    .unwrap();
    let dst = dir.path().join("mismatch-dst");
    let run_root = dir.path().join("mismatch-run");

    let output = m80()
        .args([
            "quickstart",
            "--artifact-url",
            &format!("file://{}", tarball.display()),
            "--artifact-dir",
            dst.to_str().unwrap(),
            "--run-root",
            run_root.to_str().unwrap(),
            "--no-run",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("artifact tarball sha256 mismatch"),
        "quickstart must fail before extraction on external checksum mismatch; stderr={stderr}"
    );
    assert!(
        !dst.exists(),
        "quickstart must not install artifacts after checksum mismatch"
    );
}
