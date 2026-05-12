# 0001 — Validate AF_UNIX path budget at admission, not at bind

**Status:** Accepted, 2026-05-08
**Tracked under:** `m80-o4z82.1`
**Postmortem:** `docs/postmortems/2026-05-08-af-unix-path-budget.md`

## Context

m80's Firecracker REST API socket and host-side vsock socket are bound at:

```
<run_root>/<vm_id>/<fc_basename>/<vm_id>/root/{firecracker,vsock}.sock
```

`vm_id` appears **twice** because m80-jailer inherits Firecracker's jailer
convention of nesting `<chroot-base>/<exec-basename>/<id>/root/`. The
kernel's `struct sockaddr_un` reserves 108 bytes for `sun_path` including
the null terminator → 107 usable. Caller-supplied vm_ids that produce a
path ≥ 108 bytes fail at `bind()` with `EINVAL` "AF_UNIX path too long".

Before this ADR, m80 had no admission-time validation of vm_id length. An
overflowing vm_id would surface as an `FcError::Io(io::Error)` deep inside
the launch pipeline — far from where the bad input was accepted, with
partial run-dir state already created and an admission permit already held.

## Decision

`Backend::admit()` validates the constructed socket path length **before
acquiring the admission semaphore permit**. The check is pure arithmetic
via `m80_firecracker::layout::socket_path_len(run_root, vm_id, fc_basename)`
against `m80_firecracker::layout::SUN_PATH_BUDGET = 107`. Overflow returns
`FcError::Config(ConfigError::VmIdPathBudgetExceeded { vm_id, run_root,
fc_basename, path_len, budget })`.

The check runs only when the caller supplies `vm_id` explicitly. The
auto-generated form (`vm-{pid}-{ts}` from `resolve_vm_id`) is bounded by
construction (≤ 24 bytes) and skips the check.

## Alternatives considered

**A. Catch and rewrap the IO error at bind time.** Detect EINVAL on bind
specifically, classify as the AF_UNIX-path-too-long case, remap to a typed
variant. *Rejected* because:
- Conflicts directly with the `## What we don't want` doctrine in
  `CLAUDE.md`: "No catch-and-rewrap of I/O errors that already say what
  failed". Asymmetric reclassification would hide which call site produced
  the failure.
- The failure has already happened mid-launch with partial state to clean
  up. Validate before action is cheaper and clearer.
- Different kernel versions have surfaced this as different errno values in
  the past; pattern-matching on errno is fragile.

**B. Validate at launch's phase 1 (after `resolve_vm_id`) instead of admit.**
The vm_id is "final" at that point (auto-generated values are resolved),
which is technically the latest moment all inputs are concrete. *Rejected*
because:
- Admit is the contractual entry point for caller-supplied input. Failing at
  admit means no permit was acquired and no run-dir was created; the caller
  can retry with a different vm_id immediately, with no cleanup state.
- Auto-generated vm_ids are bounded by construction; checking them at
  launch is dead code. The "single check at the natural choke point"
  principle places the check at the input boundary.
- Launching to fail wastes work (jail materialization, possibly cgroup
  creation) before the cap is hit.

**C. Truncate or hash-truncate vm_id when constructing the path.** Mangle
the input to fit instead of rejecting it. *Rejected* because:
- vm_id is the operator-visible identity throughout the lifecycle (`m80
  logs <vm_id>`, run-dir basename, diagnostics correlation, jailer
  `--id`). The path-component vm_id and the operator-visible vm_id must
  remain identical, or every consumer that walks run-dirs has to learn the
  truncation rule.
- Constraining caller input is one rule (length cap); hashing is many rules
  (input → hashed form → reverse lookup → display form). The first
  composes; the second multiplies surface.
- "No silent recovery" doctrine in `CLAUDE.md`: surface the failure, let
  the caller decide.

## Consequences

- A new `ConfigError` variant exists; the CLI's exhaustive
  `Config(_) → EXIT_CONFIG` mapping in `crates/m80-cli/src/errors.rs`
  already covers it without renumbering exit codes.
- `m80_firecracker::layout::socket_path_len` and `SUN_PATH_BUDGET` are now
  public surface and need to stay in sync with any future change to the
  jail layout. A change that adds a path component (e.g., further nesting
  the jail root) requires updating `socket_path_len` in the same diff.
- Caller-supplied vm_ids ≥ 28 bytes under `/var/lib/m80-run` (worst-case
  default `fc_basename = firecracker`) are now formally rejected. This
  matches the implicit constraint that already applied at `bind()` time;
  no caller that previously succeeded will now fail.
- Test suites that constructed long vm_ids via `unique_vm_id`-style helpers
  needed trimming. Done as part of `m80-o4z82.2`. Future tests can rely on
  the typed admission error to surface the cap during development.

## Source

2026-05-08 conversation that diagnosed six failing real-KVM tests under
five distinct error strings and traced them to one structural cause; see
the postmortem for the full diagnostic narrative.
