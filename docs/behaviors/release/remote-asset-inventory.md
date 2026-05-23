# Remote Release Asset Inventory

After upload or validate-only rerun selection, the publish job re-downloads
every public release asset and writes `m80-release-remote-assets.json`. The
inventory is the machine-readable record of the bytes that GitHub is serving,
not a restatement of local dist files.

Each row records the release asset id, asset name, kind, size, SHA256 digest,
browser download URL, `created_at`, and `updated_at`. The digest is computed
from the re-downloaded file under `/tmp/m80-release-redownload`; API metadata
alone is not accepted as proof of bytes.

The blocking identity fields are release tag, release id, asset id, asset name,
kind, size, SHA256 digest, and browser download URL. `created_at` and
`updated_at` are diagnostic fields: they must be valid timestamps so the
evidence is readable, but later verification does not fail just because GitHub
reports a harmless timestamp change or non-monotonic asset clock ordering.

The verifier consumes three inputs:

- `m80-release-upload-manifest.json`, which names the expected public assets;
- `github-release.json`, captured from `gh api repos/<owner>/<repo>/releases/tags/<tag>`;
- the re-downloaded public asset files.

The inventory fails closed when GitHub metadata has duplicate asset names or
ids, omits a manifest asset, contains an unexpected asset, reports the wrong
size, lacks a browser download URL, or when any re-downloaded file is missing
or has a stale digest. That catches API pagination/truncation gaps and local
redownload failures before later rerun or latest-promotion gates can trust the
release state. Reupload or manual replacement is still caught by identity:
asset id changes fail even when the bytes match, and digest changes fail even
when the asset name is unchanged.

Rerun preflight mode adds the local release decision inputs to that comparison:
the upload manifest, `m80-release-build.json`, and
`m80-release-publish-decision.json`. The remote inventory asset set must match
the upload manifest exactly, and every remote row's name, kind, size, and digest
must match both the upload manifest and publish receipt. The build handoff must
name the same release tag and source commit, its own downloaded bytes must match
the remote inventory row, and its `bundle_metadata_sha256` must match the
remote bundle metadata row. A stale receipt, partial upload, duplicate remote
name/id, or manual asset replacement fails before latest-promotion authority can
move. The failure prints the safe recovery path: delete the bad release or
publish a new tag; the protected publish job does not clobber or overwrite
mismatched public assets.

The inventory is uploaded as a workflow artifact named
`m80-release-remote-assets-<run_id>` and is also included in failed publish
diagnostics when it exists.
