# Image provenance manifest — write-time behaviors

Source: predecessor `infra/firecracker/prepare-guestd-image.sh`; m80 crate `m80-image-manifest`.

---

## emit {#emit}

The system writes `<output_rootfs>.manifest.json` alongside the output rootfs
at the end of the build with mode 0644.

**Present-tense statement.** `Manifest::write(path)` creates or overwrites the
file at `path`, sets POSIX mode 0644 (compiled out on non-Unix targets), and
emits canonical JSON with a trailing newline.  The conventional path is
`<output_rootfs_path>.manifest.json`; the naming is the caller's
responsibility — `m80-image-manifest` writes wherever it is told.

**predecessor source.**
- Line 10: `OUTPUT_MANIFEST_PATH="${FIRECRACKER_GUESTD_ROOTFS_MANIFEST:-${OUTPUT_ROOTFS_IMAGE}.manifest.json}"`
- Lines 243-316: heredoc written to a temp file then installed with `sudo install -D -m 0644 "${manifest_tmp}" "${OUTPUT_MANIFEST_PATH}"`.

**m80 test.** `crates/m80-image-manifest/tests/manifest_emit.rs::emits_manifest_beside_rootfs`

---

## schema-version {#schema-version}

The system stamps the manifest with `schema_version: 1` and the resolved
`expected_firecracker_version` so a boot-time validator can refuse mismatches.

**Present-tense statement.** Every manifest written by `Manifest::write`
carries `schema_version` equal to the crate constant `SCHEMA_VERSION` (value
`1`).  `Manifest::read` checks this field on deserialisation and returns
`ManifestError::UnsupportedSchemaVersion(v)` for any `v != 1`.

`expected_firecracker_version` is a free-form string (e.g., `"v1.15.1"`)
supplied by the build tool.  `m80-image-manifest` records and returns the
value; version enforcement against a live binary is the caller's
responsibility (e.g., `m80-preflight` constructs
`ManifestError::FirecrackerVersionMismatch` when the live binary differs).

**predecessor source.**
- Line 22: `MANIFEST_SCHEMA_VERSION="${FIRECRACKER_GUESTD_MANIFEST_SCHEMA_VERSION:-1}"`
- Lines 246-247: `"schema_version": ${MANIFEST_SCHEMA_VERSION}, "expected_firecracker_version": "${EXPECTED_FIRECRACKER_VERSION}"`

**m80 test.** `crates/m80-image-manifest/tests/manifest_schema_version.rs::stamps_schema_and_firecracker_version`

---

## sha256-coverage {#sha256-coverage}

The system records sha256 digests for the kernel, source rootfs, output
rootfs, daemon binary, service unit, and workspace mount unit in the manifest.

**Present-tense statement.** The `Manifest` struct carries six paired
`<artifact>_path` / `<artifact>_sha256` field pairs:

| Path field | Hash field | Artifact |
|---|---|---|
| `kernel_image` | `kernel_image_sha256` | kernel vmlinux / bzImage |
| `source_rootfs_image` | `source_rootfs_sha256` | upstream/base rootfs (squashfs or ext4) |
| `output_rootfs_image` | `output_rootfs_sha256` | the built ext4 image Firecracker mounts |
| `daemon_binary_path` | `daemon_binary_sha256` | the in-VM m80 daemon binary |
| `service_unit_path` | `service_unit_sha256` | systemd service unit file |
| `workspace_mount_path` | `workspace_mount_sha256` | systemd workspace mount unit file |

The two systemd units carry **separate** hashes — predecessor's combined
`units_sha256` is split here so a tamper of just one unit pinpoints which
file failed (`service_unit_path` vs. `workspace_mount_path`).

`Manifest::verify(root)` recomputes all six hashes from the on-disk files
(absolute paths used as-is; relative paths joined with `root`) in a single
call. There is no separate `verify_units()` method.

**predecessor source.**
- Lines 225-232: `sha256sum` over kernel, source rootfs, daemon binary,
  service file, workspace mount file, output rootfs.
- Lines 248-265: individual JSON fields including separate `guestd_service_sha256`
  and `workspace_mount_sha256`.

**m80 test.** `crates/m80-image-manifest/tests/manifest_sha256_coverage.rs::sha256_covers_all_inputs`

---

## boot-fields {#boot-fields}

The system records `boot_target`, `guest_port`, and `ready_marker` in the
manifest so the host can derive vsock and serial-probe parameters at boot.

**Present-tense statement.**

| Field | Purpose |
|---|---|
| `boot_target` | systemd target to reach (e.g., `"multi-user.target"`); passed to the boot command line |
| `guest_port` | vsock port the in-VM daemon listens on; used by the host to open the vsock connection |
| `ready_marker` | string the guest emits on the serial console when the daemon is ready; used by the host for serial probing |

These fields are informational at the manifest layer — `m80-image-manifest`
records and returns them; policy enforcement is the caller's responsibility.

**predecessor source.**
- Lines 15-17: env-var defaults for `GUESTD_BOOT_TARGET`, `GUESTD_GUEST_PORT`, `GUESTD_READY_MARKER`.
- Lines 308-310: JSON fields in the heredoc.

**m80 test.** `crates/m80-image-manifest/tests/manifest_boot_fields.rs::records_boot_target_port_marker`
