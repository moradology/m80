//! Content-addressed snapshot-template store.
//!
//! `m80-snapshot-template` owns the persisted template schema and process-local
//! pins. It does not spawn Firecracker, bind paths into a jail, or delete
//! image-store artifacts referenced by a template.

#![deny(missing_docs)]

mod error;
mod hooks;
mod identity;
mod index;
mod layout;
mod manifest;
mod paths;
mod store;

pub use error::TemplateStoreError;
pub use hooks::{HookSpec, HookSpecSet, HostnameSpec};
pub use identity::{
    ImageDigest, PmemTemplateEntry, PmemTemplateSharing, TemplateDigest, TemplateFingerprint,
    TemplateInputs, TemplateRef,
};
pub use index::{Index, IndexEntry};
pub use layout::TemplateBodyPaths;
pub use manifest::{TemplateManifest, TemplateRestoreLayout};
pub use paths::{GuestMountPath, JailBackingPath};
pub use store::{PinnedTemplate, TemplateBuildPlan, TemplatePin, TemplateStore, TemplateSummary};

/// Manifest and index schema version.
pub const SCHEMA_VERSION: u32 = 1;
