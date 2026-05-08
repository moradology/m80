# Minimal-image design (bead m80-6a0q.1)

DESIGN-only leaf. Locks the manifest schema, build config surface, and
launch dispatch *before* the m80-6a0q IMPL leaves (.2/.3/.4/.5) start.

## Goal

A second image kind alongside `ubuntu`: **`minimal`**. Busybox + statically-
linked `m80-guestd` running as PID 1, no systemd. Reduces guest cold-boot
to a fraction of the ubuntu+systemd path. Smolvm-validated approach (their
agent runs as PID 1; see `smolvm-exploration/03-boot-path-and-readiness.md`).

## Schema change — `m80-image-manifest`

`SCHEMA_VERSION` bumps `1 → 2`. Add an `image_kind` discriminator and
make systemd-specific fields kind-conditional.

```rust
pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageKind {
    Ubuntu,
    Minimal,
}

pub struct Manifest {
    pub image_kind: ImageKind,                          // NEW (required)

    // common fields (apply to both kinds):
    pub daemon_binary_path: PathBuf,
    pub daemon_binary_sha256: String,
    pub expected_firecracker_version: String,
    pub guest_port: u32,
    pub kernel_image: PathBuf,
    pub kernel_image_sha256: String,
    pub no_egress_reason: Option<String>,
    pub output_rootfs_image: PathBuf,
    pub output_rootfs_sha256: String,
    pub ready_marker: String,
    pub schema_version: u32,

    // ubuntu-only fields (None when image_kind == Minimal):
    pub source_rootfs_image: Option<PathBuf>,
    pub source_rootfs_sha256: Option<String>,
}
```

`Manifest::verify` enforces the kind/field invariant: `Ubuntu` requires
the source-rootfs `Option` fields populated; `Minimal` requires them to be
`None`. Asymmetric population is a hard error
(`ManifestError::InconsistentKind`).

No migration path from v1 → v2: per CLAUDE.md schema-version policy,
"future versions are new code, not migrations." Existing v1 manifests
fail the schema-version probe with `UnsupportedSchemaVersion(1)`; rebuild.

## Build config surface — `m80-image-build`

Extend `RootfsConfig` in `crates/m80-image-build/src/config.rs`:

```rust
pub struct RootfsConfig {
    pub size: String,
    /// "ubuntu" | "minimal". Defaults to "ubuntu" if absent (v0.1 compat
    /// for the smoke.sh config) — but new builds should declare it
    /// explicitly. Deprecated default is removed in v0.2 final.
    pub kind: Option<String>,  // parsed into ImageKind in pipeline.rs
}
```

CLI surface unchanged — `m80-image-build run --config <path>` reads the
TOML; the kind is a config field, not a flag.

## Build pipeline — `m80-image-build`

`pipeline.rs::run_build` branches at the rootfs-construction step:

| Step | Ubuntu (current) | Minimal (new) |
|---|---|---|
| Source rootfs | firecracker-ci ubuntu squashfs | None — built from scratch |
| Mount + chroot | unsquashfs into ext4, loop-mount | mkfs.ext4, loop-mount |
| Init system | m80-guestd PID 1 | m80-guestd PID 1 |
| Daemon install | binary at `/m80-guestd` | binary at `/m80-guestd` |
| /init | symlink `/init → /m80-guestd` | symlink `/init → /m80-guestd` |
| Manifest fields | populates `source_rootfs_*` | leaves `source_rootfs_*` as `None` |

Static linking of `m80-guestd`: the binary must be self-contained
(musl target or `-C target-feature=+crt-static`) so it runs under the
minimal rootfs without glibc. Out of scope for `.1` DESIGN — flagged
as a constraint for the `.2`/`.4` IMPL leaves.

## Launch dispatch — `m80-firecracker`

`launch.rs::DEFAULT_BOOT_ARGS` becomes kind-conditional. Construction
moves into a small helper:

```rust
fn boot_args_for(kind: ImageKind, config_override: Option<&str>) -> String {
    if let Some(custom) = config_override { return custom.to_owned(); }
    let base = "console=ttyS0 reboot=k panic=1 pci=off";
    match kind {
        ImageKind::Ubuntu | ImageKind::Minimal => format!("{base} init=/m80-guestd"),
    }
}
```

Phase ordering in `launch.rs` is unchanged — only the
`BootSourceConfig::boot_args` body differs. Manifest is already loaded
in phase 3 (`manifest.verify`); phase 8 (boot-source) consumes
`manifest.image_kind` to pick args.

## CLI surface — `m80-cli`

Unchanged. `m80 launch` does not learn a `--image-kind` flag in v0.1;
the manifest discloses what kind the image is, and the orchestrator
picks the right boot path. Future `m80 image build --kind minimal`
shorthand is a CLI ergonomics question, not a schema question.

## What the IMPL leaves inherit

- **m80-6a0q.2** (PID-1 hygiene): mount `/proc`, `/sys`, devtmpfs;
  reap `SIGCHLD`; handle `SIGTERM`/`SIGINT`; never panic. Cite
  `smolvm/src/agent/main.rs` boot-log + `process::exit(1)` pattern.
- **m80-6a0q.3** (workspace mount via mount(2)): `nix::mount::mount` of
  the third drive (`/dev/vdc`) onto the workspace path *after*
  pseudo-fs mounts, *before* entering the request loop.
- **m80-6a0q.4** (minimal initramfs/rootfs): mkfs.ext4 → loop-mount →
  install busybox + static `m80-guestd` → symlink `/init` → emit
  manifest with `image_kind=Minimal` and the source-rootfs fields as
  `None`.
- **m80-6a0q.5** (boot-args dispatch): land `boot_args_for` and call
  it from phase 8.

## Open questions deferred to IMPL

- **Static-link toolchain**: musl-target or stable + `+crt-static`?
  Decided in m80-6a0q.4.
- **/init vs /m80-guestd**: which path the kernel calls. Picked in
  m80-6a0q.4 (a symlink keeps both ergonomic).
- **Devtmpfs auto-mount via kernel `CONFIG_DEVTMPFS_MOUNT=y`**: if the
  firecracker-ci kernel ships it (likely yes), m80-guestd skips that
  mount and only handles `/proc` + `/sys`. Verified in m80-6a0q.2.

## Schema-version coordination

The bump from 1 → 2 means **all existing v0.1 images become invalid**
on upgrade. Coordinated landing:
1. Bump `SCHEMA_VERSION` in `m80-image-manifest`.
2. Update `m80-image-build` to write v2 manifests.
3. Update `m80-preflight` and `m80-firecracker` to read v2.
4. Single CHANGELOG entry naming the migration: "Rebuild your image."
5. No 1↔2 conversion code.
