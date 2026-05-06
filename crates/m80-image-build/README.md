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

`m80-image-build run --config <path>` dispatches on `[rootfs] kind`:

#### Ubuntu (default)

12 numbered steps:

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
12. Emit `<rootfs>.manifest.json` with `image_kind=Ubuntu` via
    `m80-image-manifest::Manifest::write`. The manifest records
    `no_egress_reason` with the shared m80 audit string because the image is
    network-neutral; runtime egress is selected at launch.

#### Minimal (`kind = "minimal"`)

10 numbered steps. No upstream squashfs; the rootfs is built from
scratch. Smaller, faster cold boot, no package manager.

1. Download kernel (same as Ubuntu).
2. Pre-allocate the output ext4 with `truncate`.
3. `mkfs.ext4 -F` against the empty file.
4. Loop-mount the output rootfs RW.
5. Copy `/bin/busybox` from the host into `<rootfs>/bin/busybox` and
   symlink common applets (`sh`, `echo`, `cat`, `ls`, `mkdir`, `mount`,
   `umount`, `stat`, `ln`, `touch`, `true`, `false`) → `busybox`. **The host
   must have `busybox-static` installed**; `apt install busybox-static`
   on Debian/Ubuntu.
6. Copy the configured `m80-guestd` binary into `<rootfs>/m80-guestd`
   and symlink `<rootfs>/init` → `/m80-guestd`. **The binary must be
   statically linked** (e.g., `cargo build --target
   x86_64-unknown-linux-musl`); a glibc-linked binary will fail at
   runtime under the busybox-only rootfs.
7. `mkdir` the PID-1 mountpoint dirs (`/workspace`, `/proc`, `/sys`,
   `/dev`, `/lower`, `/upper`, `/merged`) inside the rootfs. `/lower`,
   `/upper`, and `/merged` must exist before boot because the initial
   root is mounted read-only. Also create `/tmp` with mode `1777` for
   normal exec scratch.
8. Unmount.
9. sha256 the three artifacts that exist for Minimal kind (kernel,
   output rootfs, daemon binary).
10. Emit `<rootfs>.manifest.json` with `image_kind=Minimal` and the
    five Ubuntu-only fields (`source_rootfs_*`, `service_unit_*`,
    `workspace_mount_*`, `boot_target`) as `null`. The manifest records
    `no_egress_reason` with the shared m80 audit string because the image is
    network-neutral; runtime egress is selected at launch.

Final step: print resulting paths to stdout. **No package-manager
invocations** — `apt`/`dnf`/`pacman` are never spawned.

### Choosing an image kind

| Property                | Ubuntu                              | Minimal                             |
|-------------------------|-------------------------------------|-------------------------------------|
| Init                    | systemd                             | m80-guestd as PID 1                 |
| Userland                | Ubuntu 24.04 (full)                 | busybox + a few applets             |
| Rootfs default size     | 1 GiB                               | 256 MiB (fits in much less)         |
| Source                  | firecracker-ci squashfs             | built from scratch                  |
| Package manager         | apt available inside guest          | none                                |
| Cold boot               | slower (systemd init dominates)     | faster (m80-guestd starts directly) |
| Build network deps      | curl + S3 squashfs                  | curl only (kernel only)             |
| Static guestd required? | no (glibc dynamic OK)               | yes (musl-static; see config example)|

Pick **Ubuntu** when:
- the workload needs a familiar userspace (apt-installable tools, shared
  libraries assumed-present, `bash` features beyond busybox's `sh`)
- cold-boot latency is not the bottleneck

Pick **Minimal** when:
- launch latency matters (warm pools, agentic loops, CI burst workloads)
- you control the workload binary and can ship it self-contained
- `busybox` applets cover the inside-VM scripting needs

### Stripped kernel build

`m80-image-build kernel build [--workspace <path>]` builds the stripped
Linux 6.1.x LTS kernel via Docker:

1. `docker build -t m80-kernel-builder crates/m80-image-build/kernel-builder/`
2. `docker run --rm -v <kernels-dir>:/out m80-kernel-builder`
3. Outputs `crates/m80-image-build/kernels/vmlinux-m80-<config-sha>.bin`.

`<config-sha>` is the sha256 of the `.config` after `make olddefconfig`
(build inputs, not vmlinux output — same config always produces same name).

Files:
- `kernel-builder/Dockerfile` — ubuntu:22.04 base; build tools; determinism
  pins (`KBUILD_BUILD_TIMESTAMP=0`, `SOURCE_DATE_EPOCH=0`).
- `kernel-builder/m80-stripped.config` — canonical keep/drop config per
  `docs/design/stripped-kernel.md`. Contains `CONFIG_OVERLAY_FS=y` and
  `CONFIG_OVERLAY_FS_XINO_AUTO=y` (required by m80-f2zc.5), plus the
  cgroup/tmpfs/event primitives required for Ubuntu systemd to mount its API
  filesystems.
- `kernel-builder/build.sh` — copies config, runs `olddefconfig`, builds
  vmlinux, prints config sha, copies output to `/out`.
- `kernels/` — gitignored binary output directory.

### CLI surface

- `m80-image-build run --config <toml>` — full pipeline.
- `m80-image-build run --config <toml> --dry-run` — print what would
  happen, no I/O.
- `m80-image-build verify --rootfs <path>` — re-verify the manifest
  against the on-disk artifacts.
- `m80-image-build clean --workdir <path>` — remove intermediate
  artifacts (loop mount points, temp images).
- `m80-image-build kernel build [--workspace <path>]` — build the
  stripped kernel via Docker. `--workspace` defaults to `.` (cwd).

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

Binary-only; subcommands `run`, `verify`, `clean`, and `kernel build` (run
with `--help`).

Config file shape (`m80-image-build.toml`):
```toml
[kernel]
version = "v1.15.1"
arch = "x86_64"

[rootfs]
size = "1GiB"
# kind = "minimal"   # uncomment to build the busybox + static-guestd image

[guestd]
binary = "../../target/release/m80-guestd"
# For [rootfs] kind = "minimal", point `binary` at a static build:
# binary = "../../target/x86_64-unknown-linux-musl/release/m80-guestd"

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
- `tests/kernel_build_pipeline.rs` — config-sha computation unit tests
  (unconditional). Docker build smoke test is `#[ignore]`d; run manually
  with `cargo test -- --ignored`.

Real-build smoke tests (network + root + loop device) are run manually
with `sudo m80-image-build run --config <path>`; CI doesn't have the
required privileges.
