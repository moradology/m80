# Image Build — Provenance Manifest

Source: predecessor `infra/firecracker/prepare-guestd-image.sh`; m80 crate
`m80-image-manifest`.

## emit {#emit}

The build writes `<output_rootfs>.manifest.json` beside the output rootfs at
the end of a successful build. `Manifest::write(path)` writes pretty JSON with
a trailing newline and sets POSIX mode `0644` on Unix.

Test: `m80-image-manifest/tests/manifest_emit.rs::emits_manifest_beside_rootfs`.

## schema-version {#schema-version}

Every current manifest carries `schema_version: 3`, the crate constant
`SCHEMA_VERSION`. `Manifest::read` probes this field before full
deserialization and returns `ManifestError::UnsupportedSchemaVersion(v)` for
any unsupported value. There is no migration path inside the crate; older
images must be rebuilt for a new schema.

The manifest also records `expected_firecracker_version` from the build config.
`m80-image-manifest` records the value; live binary-version enforcement belongs
to the caller, such as preflight.

Tests:
`m80-image-manifest/tests/manifest_schema_version.rs::stamps_schema_and_firecracker_version`
and `wrong_schema_version_is_rejected`.

## sha256-coverage {#sha256-coverage}

The manifest records sha256 digests for every artifact that exists for the
image kind:

| Path field | Hash field | Artifact |
|---|---|---|
| `kernel_image` | `kernel_image_sha256` | kernel image |
| `source_rootfs_image` | `source_rootfs_sha256` | upstream/source rootfs, Ubuntu only |
| `output_rootfs_image` | `output_rootfs_sha256` | built ext4 rootfs |
| `daemon_binary_path` | `daemon_binary_sha256` | host-side audit copy of `m80-guestd` |
| `service_unit_path` | `service_unit_sha256` | systemd service unit, Ubuntu only |
| `workspace_mount_path` | `workspace_mount_sha256` | systemd workspace mount unit, Ubuntu only |

`ImageKind::Ubuntu` requires the Ubuntu-only fields to be populated.
`ImageKind::Minimal` requires them to be `None`. The invariant is enforced on
read, write, and verify.

Test:
`m80-image-manifest/tests/manifest_sha256_coverage.rs::sha256_covers_all_inputs`.

## boot-fields {#boot-fields}

The manifest records boot-adjacent fields:

| Field | Purpose |
|---|---|
| `boot_target` | systemd target for Ubuntu images; `None` for Minimal images |
| `guest_port` | vsock port the in-VM daemon listens on |
| `ready_marker` | legacy guest log/manifest marker string; not the load-bearing host readiness mechanism |
| `image_kind` | startup model: Ubuntu systemd service or Minimal PID-1 guestd |
| `kernel_kind` | kernel provenance: stock Firecracker CI kernel or stripped m80 kernel |
| `no_egress_reason` | operator audit note for network-neutral images |

Runtime launch readiness uses the inverted-ready vsock port and protocol byte;
it does not serial-probe `ready_marker`.

Tests:
`m80-image-manifest/tests/manifest_boot_fields.rs::records_boot_target_port_marker`,
`manifest_kind_invariants.rs`, `schema_v3_kernel_kind.rs`, and
`no_egress_reason.rs`.
