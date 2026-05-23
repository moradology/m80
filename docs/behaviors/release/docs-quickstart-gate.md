# Docs Quickstart Gate

Public install snippets are a tested release contract, not prose examples.
`README.md` and `docs/runbook/release.md` mark each public quickstart block with
`m80:quickstart-snippet` comments. The snippet names are:

- `latest-install`
- `pinned-install`
- `verified-install-handoff`
- `post-install-smoke`
- `freshness-check`
- `repair-status`
- `rollback-cleanup`

`scripts/quickstart_snippets.py` owns the expected snippet bodies. It derives
release URLs from `docs/behaviors/release/public-release-root.env`, and it owns
the smoke command used after installation:

```sh
m80 run -- echo hello
```

<!-- m80:quickstart-value start -->
`m80 run -- <command>` runs that process in a Firecracker microVM and returns stdout, stderr, and exit code.
<!-- m80:quickstart-value end -->

The release proof observable for the public smoke is: `m80 run -- echo hello`
exits 0 and stdout is exactly `hello`.

`scripts/test-release-url-contract.py` extracts the marked README and runbook
blocks and fails if either doc diverges from the shared snippet contract. The
same test rejects mutable `raw.githubusercontent.com` / `main` installer URLs,
artifact-only `releases/latest/download/*.tar.gz` quickstarts, and public
install URLs outside the configured `moradology/m80` repository. The only
deprecated URL exception is an explicitly marked migration-note block in
`docs/behaviors/release/legacy-quickstart-hard-cutover.md`.

Operational snippets use the same marker contract. README and runbook examples
for the read-only freshness check, first bug-report diagnostic, and rollback cleanup
must stay as copy-pasteable POSIX shell blocks from
`scripts/quickstart_snippets.py`. The runbook may explain when to use each
command, but it must not hand-maintain alternate command bodies.

The same helper builds a public command inventory from `README.md`, crate
READMEs, `docs/runbook/**`, and `docs/behaviors/**`. Fenced command blocks that
look like public install/run snippets must classify as one of:

- `common`: the latest install command or `m80 run -- echo hello` from the
  shared quickstart source
- `pinned`: the reproducible `<version>` install command from the shared source
- `verified/operator`: the verified install handoff block from the shared source
- `troubleshooting`: a concrete pinned repair command, `m80 install-status`, or
  `m80 bug-report` command in repair/troubleshooting context
- `legacy-internal`: behavior-doc references that repeat the shared commands
  only to explain the hard cutover
- `example-run`: example-local `m80 run` commands that rely on the installed
  default profile instead of `M80_KERNEL_IMAGE` / `M80_ROOTFS_IMAGE`

Unclassified `curl`, `m80 install`, `m80 quickstart`, `sudo sh install.sh`, or
`m80 run -- echo hello` command blocks fail the release URL contract tests. This
keeps secondary docs from growing stale raw-main, wrong-owner, or artifact-only
latest instructions outside the marked README/runbook snippets.

CLI help is part of the same drift surface. `crates/m80-cli/tests/help_smoke.rs`
pins `m80 quickstart --help` as an operator/test override and rejects mutable
raw-main installer URLs or artifact-only latest release URLs in both
`m80 quickstart --help` and `m80 install --help`.

Tracker text is part of the same public contract. `scripts/verify-release-tracker-policy.py`
scans release epoch titles, descriptions, acceptance criteria, and close
reasons for stale common-path claims. Public tracker prose may name latest or
pinned `install.sh`, `m80 install`, and `m80 run -- echo hello`; `m80 quickstart`
is allowed only when it is an explicit local, fixture, operator, test, override,
legacy, or internal path such as `m80 quickstart --artifact-url <url>`.

The shared latest install snippet is a release-channel template until public
release access is proven. Any docs block that shows the `latest-install`
snippet must have nearby `m80:freshness-status` markers whose generated visible
text says the public installer status and proof state. The renderer owns
pending, scaffolded fixture proof, public proven, stale, and failed wording; the
command itself still comes from the shared snippet source.

The README quickstart stays shallow by test. It must contain the fastest latest
install snippet, the `m80 run -- echo hello` smoke, the concise process-wrapper
value sentence, and the pinned install snippet. Release verification internals,
manifest/receipt details, and host policy details stay behind links to the
release runbook, troubleshooting matrix, host prerequisite behavior docs, and
operator setup docs.

Public link health is part of the same gate. The checker walks `README.md`,
`docs/runbook/release.md`, `docs/behaviors/release/*.md`, and `examples/**/*.md`.
Local Markdown links must resolve to repository files and, when they include a
fragment, to a generated heading anchor or explicit HTML anchor. Public GitHub
release URLs must classify as generated latest installer, generated pinned
installer, public proof link, pinned troubleshooting installer, direct
operator bundle, or an explicitly ignored negative example. Raw `main`
installer URLs, artifact-only latest bundle URLs, wrong-owner release URLs,
missing local files, missing anchors, and unclassified release URLs fail with a
diagnostic that names the file, line, target, classification, and repair
command.

Easy Linux install means the public release `install.sh` is the normal path and
plain `m80 run -- echo hello` is the public proof command. Manual binary or
artifact placement belongs only in advanced/operator docs with explicit matching
release and host-prerequisite caveats. The public command inventory scans
README files, release runbooks, behavior docs, and ops docs so those deeper
pages cannot reintroduce raw-main installers, artifact-only latest tarballs, or
wrong-owner release URLs.

Examples are part of that surface. `examples/**/README.md` and
`examples/**/*.sh` are scanned for public install and `m80 run` commands. They
may show concrete `m80 run` variations, but those examples must use the
installed default profile or explicitly declare fixture requirements; they must
not make `M80_KERNEL_IMAGE` / `M80_ROOTFS_IMAGE` the common path.

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
