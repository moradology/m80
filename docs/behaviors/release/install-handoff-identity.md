# Install Handoff Identity

The public release `install.sh` verifies the signed release envelope before it
extracts the bundle, but it must also verify the extracted child executable
before delegating install state changes to it.

After checksum, release-integrity, attestation, bundle digest, and bundle size
verification pass, `install.sh` extracts only `bin/m80` into a private staging
directory and runs the `bin/m80 --json version` handoff check:

```sh
bin/m80 --json version
```

The JSON output must be envelope version `1` with a `data` object whose release
identity matches the already verified release material:

- `version_status` is `release`;
- `release_build` is `true`;
- `binary_version`, `release_tag`, and `expected_release_tag` equal the pinned
  release tag;
- `source_commit` equals the signed release-integrity commit and build manifest
  `source_commit`;
- `target` equals the bundle metadata and build manifest target;
- `target_triple` is recorded in the build manifest target triples;
- `package_version` equals the bundle metadata `package_version` and build
  manifest `m80_package_version`;
- `protocol_version` equals both bundle metadata protocol fields;
- `manifest_schema_version`, `build_receipt_schema_version`, and
  `install_provenance_schema_version` equal the bundle metadata schema fields.

Any mismatch, malformed JSON, missing identity field, dev/local build identity,
or failed identity command stops before `m80 install --bundle-url ...` runs.
The final delegation uses the official release bundle URL, not the local
downloaded bundle path, so the Rust installer still takes the official-release
verification path and writes the installed release proof cache. The installer
prints the verified handoff binary version, source commit, protocol, and
manifest schema before the final delegation.

Regression coverage lives in `scripts/test-release-bundle.py`:

- `test_rendered_install_script_selects_verified_selector_before_bundle`
  covers the successful handoff identity print;
- `test_rendered_install_script_rejects_extracted_m80_source_commit_mismatch_before_install`;
- `test_rendered_install_script_rejects_extracted_m80_release_tag_mismatch_before_install`;
- `test_rendered_install_script_rejects_extracted_m80_target_mismatch_before_install`;
- `test_rendered_install_script_rejects_extracted_m80_target_triple_mismatch_before_install`;
- `test_rendered_install_script_rejects_extracted_m80_dev_identity_before_install`;
- `test_rendered_install_script_rejects_extracted_m80_malformed_identity_before_install`;
- `test_rendered_install_script_rejects_extracted_m80_missing_identity_field_before_install`;
- `test_rejects_binary_source_commit_mismatch` covers package-time rejection
  before the installer can be rendered;
- `test_rejects_binary_target_mismatch` and
  `test_rejects_binary_target_triple_mismatch` cover package-time rejection of
  child binaries whose target identity is not release-bound.
