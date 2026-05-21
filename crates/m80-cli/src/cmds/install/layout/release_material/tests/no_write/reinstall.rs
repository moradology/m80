use super::*;
use m80_image_manifest::{
    HostBinariesManifest, HostBinaryEntry, HostBinaryName, HostLaunchMaterialEntry,
    HostLaunchMaterialName,
};

#[test]
fn same_version_reinstall_with_identical_proof_material_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let active_before = fs::read_link(install_root.join("active")).unwrap();
    let profile_before = fs::read(install_root.join("profiles/default.toml")).unwrap();
    let config_before = fs::read(install_root.join("config.toml")).unwrap();
    let version_before = snapshot_install_root(&install_root.join("versions/v0.0.0"));

    let second = install_layout_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );

    let reinstall = second
        .reinstall
        .as_ref()
        .expect("same-version reinstall should report idempotency");
    assert_eq!(second.state, "already_installed");
    let json = serde_json::to_value(&second).expect("summary should serialize");
    assert_eq!(json["state"], "already_installed");
    assert_eq!(reinstall.status, "idempotent_same_material");
    assert_eq!(
        reinstall.existing_manifest_digest,
        reinstall.verified_manifest_digest
    );
    assert_eq!(second.files_copied, 0);
    assert!(!second.active_pointer_flipped);
    assert!(!second.profile_written);
    assert_eq!(second.preflight_gate, "not_run_idempotent_reinstall");
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        active_before
    );
    assert_eq!(
        fs::read(install_root.join("profiles/default.toml")).unwrap(),
        profile_before
    );
    assert_eq!(
        fs::read(install_root.join("config.toml")).unwrap(),
        config_before
    );
    assert_eq!(
        snapshot_install_root(&install_root.join("versions/v0.0.0")),
        version_before,
        "same-version no-op must not rewrite installed release bytes"
    );
    assert_no_layout_staging_dirs(&install_root, "idempotent reinstall");
}

#[test]
fn same_version_reinstall_with_changed_verifier_versions_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install_with_m80_version(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
        "v0.0.0-old-verifier",
    );

    let second = install_layout_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let reinstall = second
        .reinstall
        .as_ref()
        .expect("verifier-version-only drift should still report idempotency");

    assert_eq!(reinstall.status, "idempotent_same_material");
    assert_ne!(
        reinstall.existing_manifest_digest, reinstall.verified_manifest_digest,
        "verifier_versions remain manifest provenance but not public trust material"
    );
    assert_eq!(second.files_copied, 0);
    assert!(!second.active_pointer_flipped);
    assert!(!second.profile_written);
    assert_no_layout_staging_dirs(&install_root, "verifier-only drift reinstall");
}

#[test]
fn same_version_reinstall_with_stale_installed_byte_refuses_explicit_repair() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let active_before = fs::read_link(install_root.join("active")).unwrap();
    let stale_path = install_root.join("versions/v0.0.0/artifacts/output.ext4");
    fs::write(&stale_path, b"tampered rootfs").expect("tamper installed byte");

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let message = err.to_string();

    assert!(
        message.contains("install.reinstall"),
        "stale byte error should name reinstall field: {message}"
    );
    assert!(
        message.contains("installed byte mismatch")
            && message.contains(&stale_path.display().to_string()),
        "stale byte error should name the mismatched byte: {message}"
    );
    assert!(
        message.contains("repair_command=rm -rf --"),
        "stale byte error should name exact repair command: {message}"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        active_before
    );
    assert_eq!(
        fs::read(&stale_path).unwrap(),
        b"tampered rootfs",
        "failed reinstall must not overwrite stale byte by default"
    );
    assert_no_layout_staging_dirs(&install_root, "stale installed byte reinstall");
}

#[test]
fn same_version_reinstall_with_missing_installed_byte_refuses_explicit_repair() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let active_before = fs::read_link(install_root.join("active")).unwrap();
    let missing_path = install_root.join("versions/v0.0.0/bin/m80-net-helper");
    fs::remove_file(&missing_path).expect("remove installed byte");

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let message = err.to_string();

    assert!(
        message.contains("install.reinstall"),
        "missing byte error should name reinstall field: {message}"
    );
    assert!(
        message.contains("installed release path is unreadable")
            && message.contains(&missing_path.display().to_string()),
        "missing byte error should name the missing byte: {message}"
    );
    assert!(
        message.contains("repair_command=rm -rf --"),
        "missing byte error should name exact repair command: {message}"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        active_before
    );
    assert!(
        !missing_path.exists(),
        "failed reinstall must not recreate missing byte by default"
    );
    assert_no_layout_staging_dirs(&install_root, "missing installed byte reinstall");
}

#[test]
fn same_version_reinstall_with_missing_proof_cache_manifest_refuses_explicit_repair() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let active_before = fs::read_link(install_root.join("active")).unwrap();
    let manifest_path =
        install_root.join("versions/v0.0.0/artifacts/release-proof-cache/manifest.json");
    fs::remove_file(&manifest_path).expect("remove proof-cache manifest");

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let message = err.to_string();

    assert!(
        message.contains("proof-cache.reinstall"),
        "missing proof-cache manifest error should name reinstall field: {message}"
    );
    assert!(
        message.contains("stale proof cache")
            && message.contains(&manifest_path.display().to_string()),
        "missing proof-cache manifest error should name stale proof cache path: {message}"
    );
    assert!(
        message.contains("repair_command=rm -rf --"),
        "missing proof-cache manifest error should name exact repair command: {message}"
    );
    assert_eq!(
        fs::read_link(install_root.join("active")).unwrap(),
        active_before
    );
    assert!(
        !manifest_path.exists(),
        "failed reinstall must not recreate proof-cache manifest by default"
    );
    assert_no_layout_staging_dirs(&install_root, "missing proof-cache manifest reinstall");
}

#[test]
fn same_version_reinstall_with_stale_host_manifest_refuses_explicit_repair() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let manifest_path = install_root.join("versions/v0.0.0/artifacts/host-binaries.manifest.json");
    rewrite_host_binary_path(
        &manifest_path,
        HostBinaryName::M80NetHelper,
        &install_root
            .join("versions/v0.0.0/bin/m80")
            .display()
            .to_string(),
    );

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let message = err.to_string();

    assert!(
        message.contains("install.reinstall"),
        "stale host manifest error should name reinstall field: {message}"
    );
    assert!(
        message.contains("host-binaries manifest path mismatch for m80_net_helper"),
        "stale host manifest error should name the mismatched entry: {message}"
    );
    assert!(
        message.contains("repair_command=rm -rf --"),
        "stale host manifest error should name exact repair command: {message}"
    );
    assert_no_layout_staging_dirs(&install_root, "stale host manifest reinstall");
}

#[test]
fn same_version_reinstall_with_stale_profile_kernel_kind_refuses_explicit_repair() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let profile_path = install_root.join("profiles/default.toml");
    let mut profile = fs::read_to_string(&profile_path).expect("read profile");
    profile = profile.replace("kernel_kind = 'stock'", "kernel_kind = 'stripped'");
    fs::write(&profile_path, profile).expect("tamper profile kernel kind");

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let message = err.to_string();

    assert!(
        message.contains("install.reinstall"),
        "stale profile error should name reinstall field: {message}"
    );
    assert!(
        message.contains("installed default profile field kernel_kind mismatch"),
        "stale profile error should name kernel kind drift: {message}"
    );
    assert!(
        message.contains("repair_command=rm -rf --"),
        "stale profile error should name exact repair command: {message}"
    );
    assert_no_layout_staging_dirs(&install_root, "stale profile kernel kind reinstall");
}

#[test]
fn same_version_reinstall_with_missing_default_profile_refuses_explicit_repair() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let profile_path = install_root.join("profiles/default.toml");
    fs::remove_file(&profile_path).expect("remove installed default profile");

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let message = err.to_string();

    assert!(
        message.contains("install.reinstall"),
        "missing profile error should name reinstall field: {message}"
    );
    assert!(
        message.contains("installed default profile is unreadable")
            && message.contains(&profile_path.display().to_string()),
        "missing profile error should name the unreadable profile: {message}"
    );
    assert!(
        message.contains("repair_command=rm -rf --"),
        "missing profile error should name exact repair command: {message}"
    );
    assert!(
        !profile_path.exists(),
        "failed reinstall must not recreate missing profile by default"
    );
    assert_no_layout_staging_dirs(&install_root, "missing default profile reinstall");
}

#[test]
fn same_version_reinstall_with_missing_installed_config_refuses_explicit_repair() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let config_path = install_root.join("config.toml");
    fs::remove_file(&config_path).expect("remove installed config");

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let message = err.to_string();

    assert!(
        message.contains("install.reinstall"),
        "missing config error should name reinstall field: {message}"
    );
    assert!(
        message.contains("installed config is unreadable")
            && message.contains(&config_path.display().to_string()),
        "missing config error should name the unreadable config: {message}"
    );
    assert!(
        message.contains("repair_command=rm -rf --"),
        "missing config error should name exact repair command: {message}"
    );
    assert!(
        !config_path.exists(),
        "failed reinstall must not recreate missing config by default"
    );
    assert_no_layout_staging_dirs(&install_root, "missing installed config reinstall");
}

#[test]
fn same_version_reinstall_with_changed_predicate_refuses_silent_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions {
            wrong_commit_sha: true,
            ..ReleaseFixtureOptions::default()
        },
    );

    assert_reinstall_change_error(err, "attestation_metadata.predicate_sha256");
    assert_no_layout_staging_dirs(&install_root, "changed predicate reinstall");
}

#[test]
fn same_version_reinstall_with_changed_public_sha256s_refuses_silent_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions {
            alternate_install_script: true,
            ..ReleaseFixtureOptions::default()
        },
    );

    assert_reinstall_change_error(err, "public_sha256s");
    assert_no_layout_staging_dirs(&install_root, "changed SHA256SUMS reinstall");
}

#[test]
fn same_version_reinstall_with_changed_trust_policy_identity_refuses_silent_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    rewrite_installed_trust_policy_identity(
        &install_root,
        "repository=moradology/m80 signer=changed issuer=changed keyset_id=changed",
    );

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );

    assert_reinstall_change_error(err, "trust_policy.identity");
    assert_no_layout_staging_dirs(&install_root, "changed trust policy reinstall");
}

#[test]
fn same_version_reinstall_change_error_names_explicit_repair_version_dir() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions {
            alternate_install_script: true,
            ..ReleaseFixtureOptions::default()
        },
    );

    let message = err.to_string();
    assert_reinstall_change_error(err, "public_sha256s");
    assert!(
        message.contains(&format!(
            "version_dir={}",
            install_root.join("versions/v0.0.0").display()
        )),
        "repair diagnostic should name the protected version dir: {message}"
    );
    assert_no_layout_staging_dirs(&install_root, "explicit repair diagnostic reinstall");
}

#[test]
fn same_version_reinstall_refuses_when_existing_version_is_not_active() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let previous = install_root.join("versions/v-previous");
    fs::create_dir_all(&previous).unwrap();
    fs::remove_file(install_root.join("active")).unwrap();
    symlink(&previous, install_root.join("active")).unwrap();

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let message = err.to_string();

    assert!(
        message.contains("install.active_pointer"),
        "active-pointer mismatch should name field: {message}"
    );
    assert!(
        message.contains("existing version directory but active pointer targets a different path"),
        "active-pointer mismatch should explain ambiguity: {message}"
    );
    assert!(
        message.contains("repair_command=rm -rf --"),
        "active-pointer mismatch should name exact repair command: {message}"
    );
    assert_no_layout_staging_dirs(&install_root, "inactive same-version reinstall");
}

#[test]
fn same_version_reinstall_refuses_when_active_pointer_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("install-root");
    let bundle_bytes = write_installable_bundle_bytes(temp.path());

    seed_existing_verified_install(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let active_pointer = install_root.join("active");
    fs::remove_file(&active_pointer).expect("remove active pointer");

    let err = install_layout_error_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );
    let message = err.to_string();

    assert!(
        message.contains("install.active_pointer"),
        "missing active-pointer error should name field: {message}"
    );
    assert!(
        message.contains("active pointer is unreadable"),
        "missing active-pointer error should explain ambiguity: {message}"
    );
    assert!(
        message.contains("repair_command=rm -rf --"),
        "missing active-pointer error should name exact repair command: {message}"
    );
    assert!(
        fs::symlink_metadata(&active_pointer).is_err(),
        "failed reinstall must not recreate missing active pointer by default"
    );
    assert_no_layout_staging_dirs(&install_root, "missing active pointer reinstall");
}

fn install_layout_with_installable_bundle(
    install_root: &Path,
    bundle_bytes: &[u8],
    options: ReleaseFixtureOptions,
) -> super::super::super::super::LayoutInstallSummary {
    run_install_layout_with_installable_bundle(install_root, bundle_bytes, options)
        .expect("installable bundle should install")
}

fn install_layout_error_with_installable_bundle(
    install_root: &Path,
    bundle_bytes: &[u8],
    options: ReleaseFixtureOptions,
) -> FcError {
    run_install_layout_with_installable_bundle(install_root, bundle_bytes, options)
        .expect_err("same-version changed proof material should fail")
}

fn seed_existing_verified_install(
    install_root: &Path,
    bundle_bytes: &[u8],
    options: ReleaseFixtureOptions,
) {
    seed_existing_verified_install_with_m80_version(install_root, bundle_bytes, options, "v0.0.0");
}

fn seed_existing_verified_install_with_m80_version(
    install_root: &Path,
    bundle_bytes: &[u8],
    options: ReleaseFixtureOptions,
    m80_version: &str,
) {
    let _guard = super::super::super::super::INSTALL_PREFLIGHT_ENV_LOCK
        .lock()
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).unwrap();
    let log_path = temp.path().join("curl.log");
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_curl(&bin_dir);
    let fixture =
        write_direct_release_materials_with_bundle_bytes(&material_dir, options, bundle_bytes);

    let _path_env = EnvVarGuard::prepend_path(&bin_dir);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);
    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );
    let _expect_source_digest = EnvVarGuard::set_value(
        "M80_FAKE_GH_EXPECT_SOURCE_DIGEST",
        expected_source_digest(options),
    );

    let verified = super::super::super::verify_official_release_bundle(&fixture.bundle_url)
        .expect("verify seed release material")
        .expect("fixture URL is official release-shaped");
    let final_dir = install_root.join("versions/v0.0.0");
    fs::create_dir_all(&final_dir).expect("create final dir");
    super::super::super::super::bundle::extract_bundle(verified.bundle_path(), &final_dir)
        .expect("extract seed bundle");
    let metadata =
        super::super::super::super::metadata::read_bundle_metadata(&final_dir.join("bundle.json"))
            .expect("read seed bundle metadata");
    super::super::super::super::metadata::rewrite_installed_metadata(
        &final_dir, &final_dir, &metadata,
    )
    .expect("rewrite seed installed metadata");
    super::super::super::super::metadata::set_final_modes(&final_dir)
        .expect("set seed final modes");
    super::super::super::super::proof_cache::write_verified_release_proof_cache(
        &verified,
        &final_dir,
        m80_version,
    )
    .expect("write seed proof cache");
    write_seed_host_binaries_manifest(&final_dir);
    write_seed_profile_and_config(install_root, &final_dir);
    symlink(&final_dir, install_root.join("active")).expect("point active at seeded install");
}

fn run_install_layout_with_installable_bundle(
    install_root: &Path,
    bundle_bytes: &[u8],
    options: ReleaseFixtureOptions,
) -> Result<super::super::super::super::LayoutInstallSummary, FcError> {
    let _guard = super::super::super::super::INSTALL_PREFLIGHT_ENV_LOCK
        .lock()
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let material_dir = temp.path().join("materials");
    fs::create_dir(&material_dir).unwrap();
    let log_path = temp.path().join("curl.log");
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    write_fake_curl(&bin_dir);
    let fixture =
        write_direct_release_materials_with_bundle_bytes(&material_dir, options, bundle_bytes);

    let _path_env = EnvVarGuard::prepend_path(&bin_dir);
    let _material_env = EnvVarGuard::set("M80_FAKE_CURL_MATERIAL_DIR", &material_dir);
    let _log_env = EnvVarGuard::set("M80_FAKE_CURL_LOG", &log_path);
    let _gh_env = EnvVarGuard::set(
        "M80_RELEASE_ATTESTATION_GH",
        &fake_gh_fixture("fake-gh-attestation-supported.sh"),
    );
    let _expect_source_digest = EnvVarGuard::set_value(
        "M80_FAKE_GH_EXPECT_SOURCE_DIGEST",
        expected_source_digest(options),
    );
    let _hostless_preflight = EnvVarGuard::set_value("M80_INSTALL_HOSTLESS_FIXTURE", "1");

    super::super::super::super::install_bundle_layout(&official_release_plan(install_root)).map_err(
        |err| {
            let _fixture_url = fixture.bundle_url;
            err
        },
    )
}

fn expected_source_digest(options: ReleaseFixtureOptions) -> &'static str {
    if options.wrong_commit_sha {
        "1111111111111111111111111111111111111111"
    } else {
        "0123456789abcdef0123456789abcdef01234567"
    }
}

fn assert_reinstall_change_error(err: FcError, expected_field: &str) {
    let message = err.to_string();
    assert!(
        message.contains("same-version reinstall would replace verified trust material"),
        "unexpected reinstall error: {message}"
    );
    assert!(
        message.contains("existing_manifest_digest=")
            && message.contains("verified_manifest_digest="),
        "reinstall error should include old/new digest: {message}"
    );
    assert!(
        message.contains(
            "explicit_repair=review_changed_public_material_then_remove_version_dir_and_reinstall"
        ),
        "reinstall error should name explicit repair action: {message}"
    );
    assert!(
        message.contains(expected_field),
        "reinstall error missing changed field {expected_field:?}: {message}"
    );
}

fn rewrite_installed_trust_policy_identity(install_root: &Path, identity: &str) {
    let manifest_path =
        install_root.join("versions/v0.0.0/artifacts/release-proof-cache/manifest.json");
    let mut manifest =
        super::super::super::super::proof_cache::read_proof_cache_manifest(&manifest_path)
            .expect("read installed proof-cache manifest");
    manifest.payload.trust_policy.identity = identity.to_owned();
    manifest.manifest_digest =
        super::super::super::super::proof_cache::proof_cache_manifest_digest(&manifest.payload)
            .expect("redigest proof-cache manifest");
    let mut encoded = serde_json::to_vec_pretty(&manifest).expect("encode proof-cache manifest");
    encoded.push(b'\n');
    fs::write(&manifest_path, encoded).expect("rewrite proof-cache manifest");
}

fn rewrite_host_binary_path(manifest_path: &Path, name: HostBinaryName, path: &str) {
    let mut manifest =
        HostBinariesManifest::read(manifest_path).expect("read host-binaries manifest");
    let binary = manifest
        .binaries
        .iter_mut()
        .find(|binary| binary.name == name)
        .expect("host binary entry exists");
    binary.path = path.into();
    manifest
        .write(manifest_path)
        .expect("rewrite host-binaries manifest");
}

fn write_seed_host_binaries_manifest(final_dir: &Path) {
    HostBinariesManifest::new(
        vec![
            host_binary(HostBinaryName::Firecracker, "/opt/firecracker/firecracker"),
            host_binary(HostBinaryName::Jailer, "/opt/firecracker/jailer"),
            installed_host_binary(HostBinaryName::M80, &final_dir.join("bin/m80")),
            installed_host_binary(
                HostBinaryName::M80JailerHarden,
                &final_dir.join("bin/m80-jailer-harden"),
            ),
            installed_host_binary(
                HostBinaryName::M80NetHelper,
                &final_dir.join("bin/m80-net-helper"),
            ),
        ],
        vec![HostLaunchMaterialEntry {
            name: HostLaunchMaterialName::FirecrackerSeccompFilter,
            path: "/opt/firecracker/seccomp.json".into(),
            sha256: "b".repeat(64),
            version: "fixture".to_owned(),
        }],
    )
    .write(&final_dir.join("artifacts/host-binaries.manifest.json"))
    .expect("write seed host-binaries manifest");
}

fn host_binary(name: HostBinaryName, path: &str) -> HostBinaryEntry {
    HostBinaryEntry {
        name,
        path: path.into(),
        sha256: "a".repeat(64),
        version: "fixture".to_owned(),
    }
}

fn installed_host_binary(name: HostBinaryName, path: &Path) -> HostBinaryEntry {
    HostBinaryEntry {
        name,
        path: path.into(),
        sha256: super::super::super::super::bundle::sha256_file(path)
            .expect("hash installed host binary"),
        version: "fixture".to_owned(),
    }
}

fn write_seed_profile_and_config(install_root: &Path, final_dir: &Path) {
    let artifacts = final_dir.join("artifacts");
    fs::create_dir_all(install_root.join("profiles")).expect("create profiles dir");
    fs::write(
        install_root.join("profiles/default.toml"),
        format!(
            "artifact_dir = '{}'\n\
             kernel_image = '{}'\n\
             rootfs_image = '{}'\n\
             kernel_kind = 'stock'\n\
             guestd = '{}'\n\
             guest_manifest = '{}'\n\
             build_receipt = '{}'\n\
             install_provenance = '{}'\n\
             host_binaries_manifest = '{}'\n\
             firecracker_bin = '/opt/firecracker/firecracker'\n\
             firecracker_seccomp_filter = '/opt/firecracker/seccomp.json'\n\
             jailer_bin = '/opt/firecracker/jailer'\n\
             jailer_harden_bin = '{}'\n\
             net_helper_bin = '{}'\n\
             run_root = '{}'\n\
             release_tag = 'v0.0.0'\n\
             m80_version = 'v0.0.0'\n\
             description = 'seed installed default profile'\n",
            artifacts.display(),
            artifacts.join("vmlinux").display(),
            artifacts.join("output.ext4").display(),
            artifacts.join("m80-guestd").display(),
            artifacts.join("output.ext4.manifest.json").display(),
            artifacts.join("output.ext4.build-receipt.json").display(),
            artifacts.join("install-provenance.json").display(),
            artifacts.join("host-binaries.manifest.json").display(),
            final_dir.join("bin/m80-jailer-harden").display(),
            final_dir.join("bin/m80-net-helper").display(),
            install_root.join("run").display(),
        ),
    )
    .expect("write seed profile");
    fs::write(
        install_root.join("config.toml"),
        format!(
            "default_profile = 'default'\nrun_root = '{}'\n",
            install_root.join("run").display()
        ),
    )
    .expect("write seed config");
}
