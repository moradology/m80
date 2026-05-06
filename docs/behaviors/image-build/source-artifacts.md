# Image Build — Source Artifact Acquisition

## kernel-discovery

Ubuntu and Minimal builds download the kernel from the public Firecracker CI
artifact bucket:
`https://s3.amazonaws.com/spec.ccfc.min/firecracker-ci/<artifact_track>/<arch>/vmlinux-5.10.245`.
`artifact_track` and `arch` come from `[kernel]` in the build config and are
validated as URL-safe path components before use. The Firecracker version pin
is recorded separately in the manifest as `expected_firecracker_version`.

Source: dossier `04-infra-and-artifacts.md` § Kernel; predecessor
`infra/firecracker/prepare-guestd-image.sh` lines 24 and 41-43.

Tests: `m80-image-build/tests/dry_run_smoke.rs::dry_run_prints_steps_to_stderr_and_creates_no_output_files`
and `m80-image-build/tests/dry_run_smoke.rs::minimal_dry_run_prints_release_artifact_steps_and_creates_no_output_files`.

## rootfs-discovery

Ubuntu builds download `ubuntu-24.04.squashfs` from the same Firecracker CI
artifact bucket and convert it into an ext4 source image with `unsquashfs` and
`mkfs.ext4 -F -d`. Minimal builds skip upstream rootfs acquisition and build an
empty ext4 from scratch.

Source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 8 and 45-52.

Test: `m80-image-build/tests/dry_run_smoke.rs::dry_run_prints_steps_to_stderr_and_creates_no_output_files`.

## missing-artifact

Artifact acquisition fails closed. Failed `curl`, `unsquashfs`, `mkfs.ext4`,
`truncate`, `mount`, or `umount` commands return an error with phase context;
the build does not continue with partial or missing kernel/rootfs artifacts.
Invalid kernel URL components are rejected before any download command is
spawned.

Source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 49-57.

Tests: `m80-image-build::config::tests::parse_size_rejects_garbage` and
`m80-image-build::config::tests::build_config_rejects_url_path_components`.

## firecracker-version

The build config must specify `[kernel].version`. m80 does not probe a local
`firecracker --version` binary and does not download Firecracker or jailer
binaries as part of image construction; those are host preflight inputs. The
configured version is copied into the emitted manifest as
`expected_firecracker_version`.

Source: predecessor `infra/firecracker/prepare-guestd-image.sh` lines 23 and
59-62. m80 intentionally hard-cuts over from probing to explicit config.

Test: `m80-image-build::config::tests::build_config_requires_explicit_firecracker_version`.

## resize

The Ubuntu pipeline creates an ext4 source image, copies it to the output
rootfs path, and resizes the output rootfs to the configured target size using
`truncate -s <bytes>`. The configured size uses `GiB`, `MiB`, or `KiB` suffixes
and must be non-zero.

Source: dossier `04-infra-and-artifacts.md` § Rootfs Pipeline; predecessor
`infra/firecracker/prepare-guestd-image.sh` lines 199-201.

Tests: `m80-image-build::config::tests::parse_size_gib`,
`parse_size_mib`, `parse_size_kib`, and `parse_size_rejects_garbage`.
