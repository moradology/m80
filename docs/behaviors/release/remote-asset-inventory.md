# Remote Release Asset Inventory

After upload or validate-only rerun selection, the publish job re-downloads
every public release asset and writes `m80-release-remote-assets.json`. The
inventory is the machine-readable record of the bytes that GitHub is serving,
not a restatement of local dist files.

Each row records the release asset id, asset name, kind, size, SHA256 digest,
browser download URL, `created_at`, and `updated_at`. The digest is computed
from the re-downloaded file under `/tmp/m80-release-redownload`; API metadata
alone is not accepted as proof of bytes.

The verifier consumes three inputs:

- `m80-release-upload-manifest.json`, which names the expected public assets;
- `github-release.json`, captured from `gh api repos/<owner>/<repo>/releases/tags/<tag>`;
- the re-downloaded public asset files.

The inventory fails closed when GitHub metadata has duplicate asset names or
ids, omits a manifest asset, contains an unexpected asset, reports the wrong
size, lacks a browser download URL, or when any re-downloaded file is missing
or has a stale digest. That catches API pagination/truncation gaps and local
redownload failures before later rerun or latest-promotion gates can trust the
release state.

The inventory is uploaded as a workflow artifact named
`m80-release-remote-assets-<run_id>` and is also included in failed publish
diagnostics when it exists.
