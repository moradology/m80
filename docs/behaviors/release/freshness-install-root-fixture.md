# Freshness Install-Root Fixture

`scripts/release_freshness.py --hostless-install-fixture` extends the public
latest freshness verifier past URL and checksum checks. The fixture downloads
the public `latest/download/install.sh`, runs it with an explicit
`--install-root`, and records the installed version directory, active profile,
host-binaries manifest, command, exit status, and release API invocations in the
freshness proof.

The fixture never uses the default `/opt/m80` install root. It snapshots `/opt`
and `/etc` before and after the installer run and fails the proof if either
watched path changes. It also wraps `gh` so `gh release upload`, `gh release
edit`, `gh release delete`, `gh release create`, and mutating release-scoped
`gh api` calls are recorded as forbidden write API attempts.

The current public release still performs a live host preflight during the
final `m80 install` handoff. Operators that need a full public extraction proof
on that release can pass `--hostless-install-sudo` on a machine where
`sudo -n` is available; the proof records `privilege=sudo -n` and the observed
`preflight_gate`. CI can still exercise the fixture mechanics without network
or privilege by using local installer fixtures in `scripts/test-release-freshness.py`.
