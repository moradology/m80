# Run-Root Filesystem Advisory

`m80-preflight` emits a non-blocking `CheckRow` named `Run-root filesystem`
after the run-root path has passed the hard checks for absolute path, existing
directory, capacity, and non-`nodev` mount flags.

The advisory probes whether the run-root supports reflinks by writing a small
temporary source file under the run-root and running:

```sh
cp --reflink=always <probe-src> <probe-dst>
```

If the command succeeds, the row reports that overlay template clones can use
metadata-only CoW. If the command exits non-zero, the row reports that storage
will fall back to a full byte copy through the existing launch-path
`cp --reflink=auto --sparse=always` command. If the probe itself cannot run or
write its temporary source, the row remains passing but says the advisory is
inconclusive.

This row does not change launch behavior. It makes the fallback visible so
operators can choose a run-root filesystem deliberately.

Tests:

- `crates/m80-preflight/src/artifacts/tests.rs::run_root_reflink_probe_reports_supported_clone`
- `crates/m80-preflight/src/artifacts/tests.rs::run_root_reflink_probe_reports_full_copy_fallback_on_unsupported_clone`
- `crates/m80-preflight/src/artifacts/tests.rs::run_root_reflink_probe_failures_are_non_blocking`
