# Docs Quickstart Gate

Public install snippets are a tested release contract, not prose examples.
`README.md` and `docs/runbook/release.md` mark each public quickstart block with
`m80:quickstart-snippet` comments. The snippet names are:

- `latest-install`
- `pinned-install`
- `verified-install-handoff`
- `post-install-smoke`

`scripts/quickstart_snippets.py` owns the expected snippet bodies. It derives
release URLs from `docs/behaviors/release/public-release-root.env`, and it owns
the smoke command used after installation:

```sh
m80 run -- echo hello
```

`scripts/test-release-url-contract.py` extracts the marked README and runbook
blocks and fails if either doc diverges from the shared snippet contract. The
same test rejects mutable `raw.githubusercontent.com` / `main` installer URLs,
artifact-only `releases/latest/download/*.tar.gz` quickstarts, and public
install URLs outside the configured `moradology/m80` repository.

The same helper builds a public command inventory from `README.md`, crate
READMEs, `docs/runbook/**`, and `docs/behaviors/**`. Fenced command blocks that
look like public install/run snippets must classify as one of:

- `common`: the latest install command or `m80 run -- echo hello` from the
  shared quickstart source
- `pinned`: the reproducible `<version>` install command from the shared source
- `verified/operator`: the verified install handoff block from the shared source
- `troubleshooting`: a concrete pinned repair command in repair/troubleshooting
  context
- `legacy-internal`: behavior-doc references that repeat the shared commands
  only to explain the hard cutover

Unclassified `curl`, `m80 install`, `m80 quickstart`, `sudo sh install.sh`, or
`m80 run -- echo hello` command blocks fail the release URL contract tests. This
keeps secondary docs from growing stale raw-main, wrong-owner, or artifact-only
latest instructions outside the marked README/runbook snippets.

Release automation uses the same contract through
`scripts/write-quickstart-proof-fixture.py` and
`scripts/verify-quickstart-proof.py`: normal hostless and real-KVM quickstart
proofs must record `m80 run -- echo hello` as both the display command and argv.
Expected-nonzero auxiliary process proofs may use a different command, but they
do not replace the public echo smoke.

Do not hand-edit a public install command in docs. Change
`docs/behaviors/release/public-release-root.env` or
`scripts/quickstart_snippets.py`, run the release URL contract tests, and let
the marker comparison show every doc that needs the corresponding update.
