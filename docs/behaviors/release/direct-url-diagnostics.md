# Direct URL Diagnostics

Bead: `m80-o3uh9.15.11.6`

`m80 install --bundle-url <URL>` is an explicit development/operator override.
The normal public path remains the versioned installer shown in the README
quickstart, whose generated freshness status controls whether the latest
installer is currently public-proven.

or, after m80 is already installed from a matching release binary,
`m80 install --release-tag <tag>`.

Concrete official bundle URLs under
`https://github.com/moradology/m80/releases/download/<tag>/<bundle>.tar.gz`
are still verified as official release material. Successful direct official URL
installs report the resolved release tag, bundle asset, bundle SHA-256,
`install.sh` SHA-256, public `SHA256SUMS` SHA-256, asset-index SHA-256,
predicate SHA-256, attestation identity, source commit, the proof-cache destination,
and `proof_cache_written=true` after the verified proof cache is
written before activation. Local fixture/operator bundle installs do not write a
release proof cache.

Direct URL verifier failures include the resolved `release_tag` when the URL
classified far enough to know it, the failed `material_class` when there is a
specific release material class, and exactly one
`retry_command=m80 install --bundle-url '<url>' --install-root '<path>'`. The
retry command is a no-write retry of the same explicit operator URL; it is not a
replacement for the normal public installer path.

JSON error output carries finite `code` values for the direct URL classes that
share the broader `Config` or `UnsupportedOperation` variants:

- `direct_url_classifier`
- `release_material_fetch`
- `release_material_digest`
- `release_material_attestation`
- `release_material_stale`
- `install_no_write_rollback`

## Tests

- `failure_context_names_release_tag_material_class_and_one_retry`
- `classifier_failure_context_names_classifier_class_and_one_retry`
- `json_envelope_codes_direct_url_diagnostics`
- `official_release_verifier_accepts_complete_material_before_staging`
- `release_material_install_summary_initializes_proof_cache_fields`
