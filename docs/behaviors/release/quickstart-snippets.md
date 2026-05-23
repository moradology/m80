# Quickstart snippets

The public quickstart commands in `README.md` and `docs/runbook/release.md`
come from `scripts/quickstart_snippets.py`. The docs-command readiness lane
writes `m80-readiness-docs-command.json` with the extracted snippet bodies, the
rendered install commands for the release tag, and a digest over those inputs.

That receipt is the readiness input for docs freshness. It does not replace the
snippet tests; it records the exact docs commands that the release gate saw.
