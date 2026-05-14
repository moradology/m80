# Image Build — Provenance Manifest

Source: predecessor `infra/firecracker/prepare-guestd-image.sh`; m80 crate
`m80-image-manifest`.

## emit {#emit}

The build writes `<output_rootfs>.manifest.json` beside the output rootfs at
the end of a successful build. `Manifest::write(path)` writes pretty JSON with
a trailing newline and sets POSIX mode `0644` on Unix.

The build also writes `<output_rootfs>.build-receipt.json` after the manifest is
on disk. The receipt records the sha256 of the manifest bytes plus the
hash-bearing artifact path/hash set. `m80-preflight` requires the receipt to
match the manifest it just read.

Test: `m80-image-manifest/tests/manifest_emit.rs::emits_manifest_beside_rootfs`.

## schema-version {#schema-version}

Every current manifest carries `schema_version: 5`, the crate constant
`SCHEMA_VERSION`. `Manifest::read` probes this field before full
deserialization and returns `ManifestError::UnsupportedSchemaVersion(v)` for
any unsupported value. There is no migration path inside the crate; older
images must be rebuilt for a new schema.

The manifest also records `expected_firecracker_version` from the build config.
`m80-image-manifest` records the value; live binary-version enforcement belongs
to the caller, such as preflight.

Every current build receipt carries `schema_version: 1`, the crate constant
`BUILD_RECEIPT_SCHEMA_VERSION`. Unknown receipt fields fail closed.

Tests:
`m80-image-manifest/tests/manifest_schema_version.rs::stamps_schema_and_firecracker_version`
and `wrong_schema_version_is_rejected`, plus
`m80-image-manifest/tests/build_receipt.rs`.

## sha256-coverage {#sha256-coverage}

The manifest records sha256 digests for every artifact that exists for the
image kind:

| Path field | Hash field | Artifact |
|---|---|---|
| `kernel_image` | `kernel_image_sha256` | kernel image |
| `source_rootfs_image` | `source_rootfs_sha256` | upstream/source rootfs, Ubuntu only |
| `output_rootfs_image` | `output_rootfs_sha256` | built rootfs |
| `daemon_binary_path` | `daemon_binary_sha256` | host-side audit copy of `m80-guestd` |

`ImageKind::Ubuntu` requires the source-rootfs fields to be populated.
`ImageKind::Minimal` requires them to be `None`. The invariant is enforced on
read, write, and verify.

Test:
`m80-image-manifest/tests/manifest_sha256_coverage.rs::sha256_covers_all_inputs`.

## boot-fields {#boot-fields}

The manifest records boot-adjacent fields:

| Field | Purpose |
|---|---|
| `guest_port` | vsock port the in-VM daemon listens on |
| `ready_marker` | legacy guest log/manifest marker string; not the load-bearing host readiness mechanism |
| `image_kind` | userland family: Ubuntu rootfs or Minimal busybox rootfs; both boot PID-1 guestd |
| `kernel_kind` | kernel provenance: stock Firecracker CI kernel or stripped m80 kernel |
| `rootfs_format` | read-only base rootfs filesystem: `ext4` or `erofs` |
| `no_egress_reason` | operator audit note for network-neutral images |

Runtime launch readiness uses the inverted-ready vsock port and protocol byte;
it does not serial-probe `ready_marker`.

Tests:
`m80-image-manifest/tests/manifest_boot_fields.rs::records_port_marker`,
`records_rootfs_format`,
`manifest_kind_invariants.rs`, `schema_v3_kernel_kind.rs`, and
`no_egress_reason.rs`.
