# prep-release.sh Dry-Run Smoke

Date: 2026-05-27

The dry-run path is exercised by `scripts/test-prep-release.py` against
`tests/fixtures/prep-release/`.

Command shape:

```sh
bash scripts/prep-release.sh \
  --repo <fixture-repo> \
  --version v1.2.3 \
  --date 2026-05-27 \
  --dry-run
```

Observed output includes:

```text
prep-release dry run for v1.2.3 (2026-05-27)

CHANGELOG.md diff:
...
+## [v1.2.3] — 2026-05-27
...

Cargo.toml diff:
...
-version = "0.1.0"
+version = "1.2.3"
```

The test asserts that fixture `CHANGELOG.md` and `Cargo.toml` are unchanged
after the dry run.
