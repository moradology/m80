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

Tracker text is part of the same public contract. `scripts/verify-release-tracker-policy.py`
scans release epoch titles, descriptions, acceptance criteria, and close
reasons for stale common-path claims. Public tracker prose may name latest or
pinned `install.sh`, `m80 install`, and `m80 run -- echo hello`; `m80 quickstart`
is allowed only when it is an explicit local, fixture, operator, test, override,
legacy, or internal path such as `m80 quickstart --artifact-url <url>`.

The shared latest install snippet is a release-channel template until public
release access is proven. Any docs block that shows the `latest-install`
snippet must have nearby visible text saying public installer status is pending,
plus a nearby `m80:public-access-proof m80-o3uh9.21.7 pending` marker for the
doc gate. Once that proof is green for the promoted release, the status text
can be updated by the freshness status work; the command itself still comes
from the shared snippet source.

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
