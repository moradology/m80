//! Snapshot-template warm-pool type surface.

use std::sync::Arc;

use m80_proto::ExecRequest;
use m80_snapshot::SnapshotPaths;

pub use m80_snapshot_template::{
    HookSpec, HookSpecSet, HostnameSpec, JailBackingPath, PmemTemplateEntry, PmemTemplateSharing,
    TemplateDigest, TemplateFingerprint, TemplateInputs, TemplateRef, TemplateStore,
    TemplateStoreError,
};

/// Warm-pool fill strategy.
#[derive(Debug, Clone)]
pub enum WarmStrategy {
    /// Existing direct snapshot-restore warm-pool path.
    DirectSnapshot {
        /// Snapshot pair restored into each warm slot.
        snapshot: SnapshotPaths,
        /// Probe request that must pass before the slot enters the ready pool.
        ready_probe: ExecRequest,
    },
    /// Snapshot-template path introduced by Phase D.
    SnapshotRestore {
        /// Store that owns template lookup, build commit, pinning, and eviction.
        store: Arc<TemplateStore>,
        /// Ordered hooks associated with the template identity and restore gate.
        hooks: HookSpecSet,
        /// Probe request that must pass before the slot enters the ready pool.
        ready_probe: ExecRequest,
    },
}

impl WarmStrategy {
    /// Construct the existing direct snapshot-restore strategy.
    #[must_use]
    pub fn direct_snapshot(snapshot: SnapshotPaths, ready_probe: ExecRequest) -> Self {
        Self::DirectSnapshot {
            snapshot,
            ready_probe,
        }
    }

    /// Construct the snapshot-template strategy.
    #[must_use]
    pub fn snapshot_restore(
        store: Arc<TemplateStore>,
        hooks: HookSpecSet,
        ready_probe: ExecRequest,
    ) -> Self {
        Self::SnapshotRestore {
            store,
            hooks,
            ready_probe,
        }
    }
}

#[cfg(test)]
mod tests;
