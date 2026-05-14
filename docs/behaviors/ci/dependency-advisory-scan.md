# Dependency Advisory Scan

Behavior capture for bead `m80-8emae.13`.

## Contract

CI runs a RustSec advisory scan independently from the workspace build/test job.
The scan reads `Cargo.lock` and fails the `audit` job on known vulnerability
advisories reported by `cargo audit`.

The scan is intentionally separate from `cargo test --workspace`:

- dependency advisories can fail fast without compiling m80
- build/test failures do not hide advisory failures
- advisory database fetches do not perturb the normal test cache key

`cargo audit` warnings such as unmaintained transitive crates are reported by
the tool. They become failing policy only when the audit configuration or
RustSec severity class requires failure.

## Nix Version Follow-Up

The audit found four `nix` versions in the resolved dependency graph:

```text
nix 0.28.0 via portable-pty
nix 0.29.0 via m80 workspace dependencies and tun
nix 0.30.1 via rtnetlink
nix 0.31.2 via vsock
```

That consolidation is intentionally split into a separate dependency-policy
bead. It requires replacing or upgrading upstream dependencies with incompatible
`0.x` `nix` requirements; forcing it in the CI-advisory change would couple a
low-risk process gate to larger runtime dependency risk.

## Verification

- `cargo audit`
- CI workflow YAML includes an `audit` job using `rustsec/audit-check@v2`
