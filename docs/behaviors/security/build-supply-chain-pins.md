# Build Supply Chain Pins

Behavior capture for bead `m80-8emae.33`.

## Quickstart Tarball Integrity

`m80 quickstart --artifact-url <url>` downloads both the release tarball and the
sibling `<url>.sha256` file. The outer tarball sha256 is verified before
extraction, then the extracted `SHA256SUMS` file is verified before artifacts are
installed. The extracted checksum file is still useful for per-file integrity,
but it is no longer trusted to certify the tarball that carried it.

## Protobuf Compiler

`m80-proto` prefers a caller-supplied `PROTOC`. If `PROTOC` is unset and the
build falls back to the vendored Linux x86_64 compiler, the build script checks
the vendored binary sha256 before executing it.

## Stripped Kernel Builder

The stripped-kernel Docker builder installs packages from a dated Ubuntu
snapshot and fetches the Linux source by immutable commit instead of mutable tag
name. The default commit is the peeled `v6.1.134` commit.

Verification:

- `crates/m80-cli/tests/quickstart_smoke.rs::quickstart_no_run_installs_verified_artifacts`
- `crates/m80-cli/tests/quickstart_smoke.rs::quickstart_rejects_tarball_when_external_checksum_mismatches`
- `crates/m80-cli/tests/quickstart_smoke.rs::quickstart_rejects_bundled_host_binaries_manifest`
- `crates/m80-cli/tests/quickstart_smoke.rs::quickstart_rejects_nested_bundled_host_binaries_manifest`
- `crates/m80-image-build/tests/kernel_build_pipeline.rs::kernel_builder_fetches_pinned_commit_not_mutable_tag`
- `crates/m80-image-build/tests/kernel_build_pipeline.rs::kernel_builder_uses_snapshot_apt_sources`
- `cargo check -p m80-proto`
