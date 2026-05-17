# Run-Root Filesystem Advisory

`m80-preflight` emits a non-blocking `CheckRow` named `Run-root filesystem`
after the run-root path has passed the hard checks for absolute path, existing
directory, capacity, and non-`nodev` mount flags.

The advisory probes whether the run-root supports reflinks by writing a small
temporary source file under the run-root and running:

```sh
cp --reflink=always <probe-src> <probe-dst>
```

If the command succeeds, the row reports that explicit reflink overlay clone
mode can use metadata-only CoW. If the command exits non-zero, the row reports
that callers should choose explicit byte-copy mode, or explicit auto mode will
select byte-copy. If the probe itself cannot run or write its temporary source,
the row remains passing but says the advisory is inconclusive; explicit auto
mode treats that as a hard selection failure.

This row does not change launch behavior. It makes the run-root's reflink
capability visible so operators can choose a filesystem and clone policy
deliberately.

Tests:

- `crates/m80-preflight/src/artifacts/tests.rs::run_root_reflink_probe_reports_supported_clone`
- `crates/m80-preflight/src/artifacts/tests.rs::run_root_reflink_probe_reports_unavailable_reflink_on_unsupported_clone`
- `crates/m80-preflight/src/artifacts/tests.rs::run_root_reflink_probe_failures_are_non_blocking`
