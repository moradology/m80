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

`m80-image-build run --config <path>` performs:

1. Resolve kernel: download from
   `https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/<version>/<arch>`,
   or use a local override path.
2. Resolve source rootfs: download the firecracker-ci squashfs, convert
   to ext4, resize to the configured size (default 1 GiB) via `truncate`.
3. Mount the ext4 via loop device (the m80 process must hold `CAP_SYS_ADMIN` or run as root; `m80-preflight` verifies this at startup).
4. Chroot install: copy `m80-guestd` into `/usr/local/bin/`, install
   the systemd units (`m80-guestd.service`, the workspace mount unit),
   create the workspace mount point, enable units in
   `multi-user.target.wants/`. **No package manager invocations.**
5. Unmount.
6. sha256 every artifact (kernel, source rootfs, output rootfs, daemon
   binary, units) and emit `<rootfs>.manifest.json` via
   `m80-image-manifest`.
7. Print the resulting paths and pass-through hashes.

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

- `m80-image-manifest` — manifest schema + writer.
- (no internal m80 privilege deps — the build process holds the required caps directly)
- `serde`, `serde_json`, `sha2`, `hex`.
- `thiserror`, `anyhow`, `tracing`.

## Tests

- Dry-run determinism: the printed plan is byte-stable across runs for
  the same config.
- Manifest equivalence: a real build and a fresh build of the same
  inputs produce equal manifests.
- Verify-fail: tampering with one artifact byte and running `verify`
  produces the right typed error.
- No-package-manager guard: the chroot stage runs with `apt`/`dnf`/etc
  removed from `PATH`; the build still succeeds.
