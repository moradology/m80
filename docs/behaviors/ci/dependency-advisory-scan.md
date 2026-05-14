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

The audit found four `nix` versions in the resolved dependency graph. This is
currently a documented upstream-blocker state, not an accidental workspace
drift:

```text
nix 0.28.0 via portable-pty
nix 0.29.0 via m80 workspace dependencies and tun
nix 0.30.1 via rtnetlink
nix 0.31.2 via vsock
```

The blockers are:

- `portable-pty 0.9.0` is the latest published `portable-pty` crate and depends
  on `nix 0.28`.
- `rtnetlink 0.21.0` is the latest published `rtnetlink` crate and depends on
  `nix 0.30.0`.
- `vsock 0.5.4` is the latest published `vsock` crate and depends on
  `nix 0.31.2`.
- `tun 0.7.13` already shares the workspace's `nix 0.29` line. Later tested
  `tun` releases (`0.7.17` through `0.8.9`) require Edition 2024 and fail to
  parse on the workspace Rust 1.82 toolchain.

Do not use a `[patch.crates-io]` override to force one `nix` version across
these crates without source-verifying each upstream call site. `nix` is still
pre-1.0 and minor releases carry API movement; a resolver trick would only hide
the dependency-policy problem.

## Verification

- `cargo audit`
- `cargo tree -i nix@0.28.0`
- `cargo tree -i nix@0.29.0`
- `cargo tree -i nix@0.30.1`
- `cargo tree -i nix@0.31.2`
- `cargo search portable-pty --limit 3`
- `cargo search rtnetlink --limit 3`
- `cargo search vsock --limit 3`
- `cargo search tun --limit 20`
- `cargo info tun@0.7.17` and `cargo info tun@0.8.9` fail on Rust 1.82 with
  `feature edition2024 is required`
- CI workflow YAML includes an `audit` job using `rustsec/audit-check@v2`
