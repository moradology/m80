# Release Notes Proof

`m80-6egdo.7` closes only after a real future stable release proves that GitHub
Release notes came from committed, human-reviewed `CHANGELOG.md` text.

After the tag workflow has published the release, capture the proof:

```sh
python3 scripts/verify-release-notes-proof.py \
  --tag vX.Y.Z \
  --workflow-run-url https://github.com/moradology/m80/actions/runs/<run-id> \
  --human-reviewed-by "<name>" \
  --out docs/release/vX.Y.Z-proof.md
```

The script reads the live body with:

```sh
gh release view vX.Y.Z --repo moradology/m80 --json body --jq .body
```

It refuses to write the proof unless the live body exactly matches the committed
`CHANGELOG.md` section body for the tag, is longer than 200 characters, contains
a Keep-a-Changelog category heading, and does not match the old one-line
`release artifacts from .../actions/runs/...` fallback.

For offline diagnosis, capture the release body separately and pass
`--release-body-file`.
