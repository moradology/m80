# `m80-image-build`

Build-time tool that produces the m80 guest image: pulls the kernel,
prepares the rootfs, installs the m80 daemon and systemd units, emits
the provenance manifest. Run this before `m80-cli launch` ever sees a
VM.

## Reason for being

The predecessor build is a 326-line bash script (`prepare-guestd-image.sh`)
mixing curl, mount, debootstrap, systemd, and cargo. It works, but it
fails opaquely and is hard to dry-run. Rewriting it as a Rust binary
gives us:

- Typed errors per phase (download / mount / chroot install / manifest
  emit) instead of "exit 1".
- A `--dry-run` mode that prints what *would* run without touching the
  filesystem.
- Reuse of `m80-image-manifest` for the provenance schema, so build and
  boot can never disagree.
- A clean opt-out from predecessor's interpreter opinions: no Python/Node,
  no `npm`/`pip` policy.

## Black-box contract

### Pipeline

`m80-image-build run --config <path>` runs 12 numbered steps (labels
emitted to stderr in `--dry-run`):

1. Download kernel from `https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/<version>/<arch>` via `curl`.
2. Download source rootfs squashfs from the same firecracker-ci bucket.
3. Convert squashfs → ext4 (in a temp dir) via `unsquashfs` + `mkfs.ext4`.
4. Resize the output ext4 to the configured size via `truncate`.
5. Loop-mount the output rootfs RW. (The m80 process must hold
   `CAP_SYS_ADMIN` or run as root; `m80-preflight` verifies this at
   startup.)
6. Copy `m80-guestd` into `<rootfs>/usr/local/bin/`.
7. Write the embedded `m80-guestd.service` into `<rootfs>/etc/systemd/system/`.
8. Write the embedded workspace mount unit alongside it.
9. `mkdir <rootfs>/workspace` and enable both units by symlinking into
   `multi-user.target.wants/`.
10. Unmount the rootfs.
11. sha256 every artifact (kernel, source rootfs, output rootfs, daemon
    binary, service unit, workspace-mount unit).
12. Emit `<rootfs>.manifest.json` via `m80-image-manifest::Manifest::write`.

Final step: print resulting paths to stdout. **No package-manager
invocations** — `apt`/`dnf`/`pacman` are never spawned.

### CLI surface

- `m80-image-build run --config <toml>` — full pipeline.
- `m80-image-build run --config <toml> --dry-run` — print what would
  happen, no I/O.
- `m80-image-build verify --rootfs <path>` — re-verify the manifest
  against the on-disk artifacts.
- `m80-image-build clean --workdir <path>` — remove intermediate
  artifacts (loop mount points, temp images).

### Failure modes

- Each phase can fail with a typed error preserved through `anyhow`
  contexts. Exit code 1 plus stderr-rendered hint.
- Any sha256 mismatch during verification is fatal; build outputs are
  marked `.tainted` and refused.

### Reproducibility

- Given the same config (kernel version, rootfs source, daemon binary
  hash, unit set), two builds produce byte-identical manifests. Tested
  in CI.
- The builder does **not** alter timestamps inside the rootfs. We use
  fixed mtimes (`SOURCE_DATE_EPOCH`) to make the output bit-reproducible.

## Public surface

This crate ships only a binary; no library API is exposed.

CLI subcommands:
- `run`, `verify`, `clean`.

Config file shape (`m80-image-build.toml`):
```toml
[kernel]
version = "v1.15.1"
arch = "x86_64"

[rootfs]
size = "1GiB"
source = "firecracker-ci"   # or "local" with a path

[guestd]
binary = "../../target/release/m80-guestd"

[output]
dir = "/opt/m80/artifacts"
```

## Non-goals

- **No package-manager-driven image builds.** No apt/dnf/pacman.
- **No interpreter installs.** Python, Node, Ruby — all out of scope.
  Adapter layers may build their own opinionated images on top.
- **No multi-arch cross-build.** This binary builds for the host's
  arch. Cross-arch builds are a v0.2 concern.
- **No image signing.** sha256 is integrity, not authenticity.
- **No image registry push.** The output is a directory of artifacts;
  shipping them elsewhere is a deployer concern.

## Dependencies

- `m80-proto` — canonical `GUEST_PORT_DEFAULT` and `READY_MARKER_DEFAULT` constants.
- `m80-image-manifest` — manifest schema + writer.
- (no internal m80 privilege deps — the build process holds the required caps directly)
- `serde`, `serde_json`, `sha2`, `hex`, `toml`.
- `thiserror`, `anyhow`, `tracing`, `tempfile`.

## Tests

- `tests/dry_run_smoke.rs` — `run --dry-run` exits 0, prints labels for
  all 12 numbered pipeline steps to stderr, creates no output dir, and
  is deterministic across invocations.
- `tests/verify_with_fixture_manifest.rs` — `verify --rootfs <path>`
  passes for a manually-constructed fixture rootfs + manifest, and fails
  with the right error when one artifact byte is tampered.

Real-build smoke tests (network + root + loop device) are run manually
with `sudo m80-image-build run --config <path>`; CI doesn't have the
required privileges.
