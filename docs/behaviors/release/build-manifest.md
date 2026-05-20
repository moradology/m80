# Release Build Manifest

The release dist publishes `m80-release-build.json` beside the bundle,
metadata, selector, and integrity predicate. It is the machine-readable record
of what built the release bytes.

The v1 manifest records:

- `release_tag` and `source_commit`;
- `rust_toolchain`;
- `target` plus every Rust `target_triple` built for the release;
- `cargo_lock_sha256`;
- `m80_package_version` and `image_kind`;
- `builder_identity` and `builder_os_image`;
- either `apt_packages` as `name`/`version` rows or a `container_digest`;
- `bundle_metadata_name` and `bundle_metadata_sha256`.

For the default Linux release, `target_triples` must include
`x86_64-unknown-linux-musl` because the guest daemon is built for that target.
The GitHub release workflow also records the host `rustc` triple used for the
host-side m80 binaries.

`scripts/verify-release-bundle.py --verify-sidecars` rejects a build manifest
whose release tag, target, image kind, package version, bundle metadata digest,
`Cargo.lock` digest, source commit, or Rust toolchain does not match the rest
of the release dist. `scripts/verify-release-integrity.py` also requires the
build manifest and checksum sidecar to be subjects in
`m80-release-integrity.json`, then checks the same manifest identity against
the signed predicate.

The versioned `install.sh` downloads `m80-release-build.json` and
`m80-release-build.json.sha256` before extracting the selected bundle. Its
embedded verifier rejects a build manifest whose release tag, source commit,
Rust toolchain, target, package version, image kind, target triples, builder
material, or bundle metadata hash does not match the signed predicate and the
selected bundle metadata. That keeps the no-installed-binary path from treating
the manifest as a mere signed blob.
