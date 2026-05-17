//! Canonical tracing span names for m80 VM-mechanics surfaces.
//!
//! These constants intentionally cover VM mechanics only: image build, pmem
//! attach, guest DAX mount, template build, template restore, and typed
//! post-restore hooks. Adapter-level agent semantics belong outside m80.

/// One structured field expected on a canonical tracing span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanField {
    /// Stable field name used by emitters.
    pub name: &'static str,
    /// Human-readable field meaning for operators and docs.
    pub description: &'static str,
}

/// One canonical tracing span and its field schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanSpec {
    /// Stable span name.
    pub name: &'static str,
    /// Operator-facing purpose of the span.
    pub description: &'static str,
    /// Structured fields expected when the emitter has the value available.
    pub fields: &'static [SpanField],
    /// Code locations expected to emit the span.
    pub emit_sites: &'static [&'static str],
}

/// Host-side image build span.
///
/// Fields: `image_digest`, `image_kind`, `source_digest`, `output_bytes`,
/// and `duration_us` when known by the emitter.
pub const M80_SPAN_IMAGE_BUILD: &str = "m80.image.build";

/// Host-side Firecracker pmem attach span.
///
/// Fields: `vm_id`, `slot`, `image_digest`, `sharing_mode`, `backing_path`,
/// and `mount_path` when known by the emitter.
pub const M80_SPAN_PMEM_ATTACH: &str = "m80.pmem.attach";

/// Guest-side erofs DAX mount span.
///
/// Fields: `device`, `mount_path`, `fs_type`, `dax_mode`, and
/// `image_digest` when known by the emitter.
pub const M80_SPAN_GUEST_DAX_MOUNT: &str = "m80.guest.dax_mount";

/// Host-side snapshot-template build span.
///
/// Fields: `template_fingerprint`, `template_body`, `pmem_layer_count`,
/// `hook_spec_digest`, and `duration_us` when known by the emitter.
pub const M80_SPAN_TEMPLATE_BUILD: &str = "m80.template.build";

/// Host-side snapshot-template restore span.
///
/// Fields: `template_fingerprint`, `vm_id`, `restore_nonce`,
/// `snapshot_body`, and `duration_us` when known by the emitter.
pub const M80_SPAN_TEMPLATE_RESTORE: &str = "m80.template.restore";

/// Guest-side typed post-restore hook execution span.
///
/// Fields: `template_fingerprint`, `restore_nonce`, `hook_index`,
/// `hook_variant`, `outcome`, and `duration_us` when known by the emitter.
pub const M80_SPAN_POST_RESTORE_HOOK: &str = "m80.post_restore_hook";

const IMAGE_BUILD_FIELDS: &[SpanField] = &[
    SpanField {
        name: "image_digest",
        description: "content digest of the produced image",
    },
    SpanField {
        name: "image_kind",
        description: "image format or role, such as erofs rootfs or pmem layer",
    },
    SpanField {
        name: "source_digest",
        description: "digest of the source artifact set when available",
    },
    SpanField {
        name: "output_bytes",
        description: "size of the produced image in bytes",
    },
    SpanField {
        name: "duration_us",
        description: "image build duration in microseconds",
    },
];

const PMEM_ATTACH_FIELDS: &[SpanField] = &[
    SpanField {
        name: "vm_id",
        description: "opaque VM id for the Firecracker instance",
    },
    SpanField {
        name: "slot",
        description: "zero-based pmem device slot",
    },
    SpanField {
        name: "image_digest",
        description: "content digest of the erofs image attached as pmem",
    },
    SpanField {
        name: "sharing_mode",
        description: "declared PmemSharing mode, for example PerVm or Shared",
    },
    SpanField {
        name: "backing_path",
        description: "host backing path supplied to Firecracker",
    },
    SpanField {
        name: "mount_path",
        description: "guest mount path requested for the pmem layer",
    },
];

const GUEST_DAX_MOUNT_FIELDS: &[SpanField] = &[
    SpanField {
        name: "device",
        description: "guest block device path, such as /dev/pmem0",
    },
    SpanField {
        name: "mount_path",
        description: "guest mount path accepted by m80",
    },
    SpanField {
        name: "fs_type",
        description: "filesystem type mounted by guestd",
    },
    SpanField {
        name: "dax_mode",
        description: "DAX option observed after mount",
    },
    SpanField {
        name: "image_digest",
        description: "content digest paired with the mount when available",
    },
];

const TEMPLATE_BUILD_FIELDS: &[SpanField] = &[
    SpanField {
        name: "template_fingerprint",
        description: "stable fingerprint of the template inputs",
    },
    SpanField {
        name: "template_body",
        description: "content-addressed template body identifier",
    },
    SpanField {
        name: "pmem_layer_count",
        description: "number of pmem layers included in the template",
    },
    SpanField {
        name: "hook_spec_digest",
        description: "digest of the typed post-restore hook set",
    },
    SpanField {
        name: "duration_us",
        description: "template build duration in microseconds",
    },
];

const TEMPLATE_RESTORE_FIELDS: &[SpanField] = &[
    SpanField {
        name: "template_fingerprint",
        description: "fingerprint selected for restore",
    },
    SpanField {
        name: "vm_id",
        description: "opaque VM id for the restored Firecracker instance",
    },
    SpanField {
        name: "restore_nonce",
        description: "host-provided nonce sent to post-restore hooks",
    },
    SpanField {
        name: "snapshot_body",
        description: "content-addressed snapshot body loaded by Firecracker",
    },
    SpanField {
        name: "duration_us",
        description: "restore operation duration in microseconds",
    },
];

const POST_RESTORE_HOOK_FIELDS: &[SpanField] = &[
    SpanField {
        name: "template_fingerprint",
        description: "fingerprint of the template that produced the lease",
    },
    SpanField {
        name: "restore_nonce",
        description: "host-provided nonce for this restore",
    },
    SpanField {
        name: "hook_index",
        description: "zero-based index within the HookSpecSet",
    },
    SpanField {
        name: "hook_variant",
        description: "closed HookSpec variant being executed",
    },
    SpanField {
        name: "outcome",
        description: "typed hook result such as ok or error variant",
    },
    SpanField {
        name: "duration_us",
        description: "hook execution duration in microseconds",
    },
];

/// Complete catalog of canonical m80 tracing spans.
pub const ALL_SPANS: &[SpanSpec] = &[
    SpanSpec {
        name: M80_SPAN_IMAGE_BUILD,
        description: "build a host-side image artifact",
        fields: IMAGE_BUILD_FIELDS,
        emit_sites: &[
            "crates/m80-image-build",
            "future m80-cli image build surface",
        ],
    },
    SpanSpec {
        name: M80_SPAN_PMEM_ATTACH,
        description: "attach a host erofs image to a Firecracker pmem device",
        fields: PMEM_ATTACH_FIELDS,
        emit_sites: &[
            "crates/m80-firecracker/src/lifecycle/exec.rs",
            "crates/m80-firecracker storage preparation",
        ],
    },
    SpanSpec {
        name: M80_SPAN_GUEST_DAX_MOUNT,
        description: "mount an erofs pmem device with DAX inside the guest",
        fields: GUEST_DAX_MOUNT_FIELDS,
        emit_sites: &["crates/m80-guestd"],
    },
    SpanSpec {
        name: M80_SPAN_TEMPLATE_BUILD,
        description: "capture a reusable snapshot-template body",
        fields: TEMPLATE_BUILD_FIELDS,
        emit_sites: &["crates/m80-firecracker/src/warm_pool/template_build.rs"],
    },
    SpanSpec {
        name: M80_SPAN_TEMPLATE_RESTORE,
        description: "restore a warm-pool slot from a snapshot-template body",
        fields: TEMPLATE_RESTORE_FIELDS,
        emit_sites: &[
            "crates/m80-firecracker/src/launch.rs",
            "crates/m80-firecracker/src/warm_pool/fill_worker.rs",
        ],
    },
    SpanSpec {
        name: M80_SPAN_POST_RESTORE_HOOK,
        description: "execute one closed typed post-restore hook",
        fields: POST_RESTORE_HOOK_FIELDS,
        emit_sites: &[
            "crates/m80-guestd",
            "crates/m80-firecracker post-restore hook request path",
        ],
    },
];
