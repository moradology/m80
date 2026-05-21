# Freshness Status Reader

Behavior bead: `m80-o3uh9.16.9.1`.

The installed-version freshness reader consumes the scheduled freshness status
artifact, not a mutable scrape of a release page. The artifact is
`m80-latest-freshness-proof.json`, uploaded by
`.github/workflows/latest-freshness.yml` after running:

```sh
python3 scripts/release_freshness.py --docs-root . --json --proof-out "$FRESHNESS_PROOF"
```

The reader accepts schema `1` success artifacts with
`freshness_network_bounded: true`, `repository: "moradology/m80"`, a stable
`resolved_tag` (`vMAJOR.MINOR.PATCH`), and a UTC RFC3339 `published_at`
timestamp. The checked URL inventory and public asset inventory must be present
and nonempty so the status came from the same docs and CI freshness lane that
guards the public install surface.

The comparison result is finite:

- `current`
- `outdated`
- `unknown_offline`
- `stale_latest_metadata`
- `prerelease_active`
- `ineligible_active`
- `local_dev_active`

The offline fallback state is `unknown_offline`. Missing status metadata,
private-network failure, or an unavailable cache must not guess that the active
install is current or stale. Malformed status metadata fails closed instead of
becoming `unknown_offline`.

This reader does not download release bundles, fetch GitHub by itself, rewrite
the active pointer, or mutate install-root state. Later command wiring may fetch
or cache a status artifact, but this comparison layer only parses the status
artifact it was handed and compares it to the active install version.
