# Snapshot Version Validation

Behavior capture for `m80-2ggw.4.1`.

Snapshot restore has two Firecracker version gates:

1. `restore()` reads `snapshot-manifest.json` and verifies the manifest's
   `expected_firecracker_version` equals the caller's restore expectation.
2. `restore()` then calls Firecracker `GET /version` on the restore-target API
   socket. Firecracker's API returns the raw Cargo version (`1.15.1`), while
   m80's preflight and manifests use the `firecracker --version` pin form
   (`v1.15.1`), so restore converts the API value to the m80 pin form before
   comparing it with the expectation.

If the live version differs, restore returns
`SnapshotError::VersionMismatch { expected, actual }`, with both fields in
m80's `v`-prefixed pin form. The failure happens before stale `vsock_uds`
unlink and before `PUT /snapshot/load`, so a mismatched Firecracker binary
cannot mutate restore-local state or ask Firecracker to interpret an
incompatible snapshot.

`restore_preverified()` skips the manifest/artifact hash gate for callers that
already verified an immutable snapshot body, but it still performs the live
`GET /version` check before unlinking or loading.

Verification:

- `crates/m80-firecracker-client/tests/snapshot.rs::get_version_sends_correct_url_and_decodes_response`
- `crates/m80-firecracker-client/tests/snapshot.rs::get_version_400_returns_version_read_failed`
- `crates/m80-snapshot/tests/version_validation.rs::version_mismatch_is_rejected_before_load`
- `crates/m80-snapshot/tests/version_validation.rs::version_match_proceeds_to_load`
