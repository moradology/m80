use super::*;

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

    let second = install_layout_with_installable_bundle(
        &install_root,
        &bundle_bytes,
        ReleaseFixtureOptions::default(),
    );

    let reinstall = second
        .reinstall
        .as_ref()
        .expect("same-version reinstall should report idempotency");
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
    assert_no_layout_staging_dirs(&install_root, "inactive same-version reinstall");
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
    fs::create_dir_all(final_dir.join("artifacts")).expect("create installed artifacts");
    super::super::super::super::proof_cache::write_verified_release_proof_cache(
        &verified,
        &final_dir,
        m80_version,
    )
    .expect("write seed proof cache");
    fs::create_dir_all(install_root.join("profiles")).expect("create profiles dir");
    fs::write(
        install_root.join("profiles/default.toml"),
        b"description = \"seed profile\"\n",
    )
    .expect("write seed profile");
    fs::write(
        install_root.join("config.toml"),
        b"default_profile = 'default'\n",
    )
    .expect("write seed config");
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
