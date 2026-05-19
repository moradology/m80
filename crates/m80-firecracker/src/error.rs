//! [`FcError`] — top-level error sum for `m80-firecracker`.

use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

use m80_cgroup::CgroupError;
use m80_firecracker_client::ClientError;
use m80_image_manifest::ManifestError;
use m80_jailer::JailerError;
use m80_net_outbound::{NetError, NetworkHelperFailureKind};
use m80_preflight::PreflightError;
use m80_proto::{DriveHotplugError, ExecStatus, FileError, HookError, PmemMountError};
use m80_snapshot::SnapshotError;
use m80_snapshot_template::TemplateStoreError;
use m80_storage::StorageError;
use m80_vsock::VsockError;

/// Structured cause for a peer disconnect before m80 observed the required
/// terminal response frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DisconnectCause {
    /// The recorded Firecracker process was no longer live when the disconnect
    /// or failed channel setup was observed.
    #[error("firecracker process dead")]
    FcProcessDead,
    /// The host could not establish or send the initial request frame over the
    /// guest vsock bridge while Firecracker still appeared live.
    #[error("guest uds connection failed")]
    UdsConnectFailed,
    /// The guest-side channel closed after the request was in flight while
    /// Firecracker still appeared live.
    #[error("mid-stream eof")]
    MidStreamEof,
    /// The peer closed intentionally before a terminal frame was required.
    #[error("clean requested close")]
    CleanRequestedClose,
}

/// Host-side wire protocol failures after a vsock channel is open.
#[derive(Debug, thiserror::Error)]
pub enum WireProtocolError {
    /// Peer sent bytes that could not be decoded as an m80 protobuf envelope.
    #[error("malformed peer frame: {0}")]
    MalformedPeer(String),
    /// Peer announced or encoded a protobuf frame larger than the active cap.
    #[error("oversized frame: {size} bytes exceeds limit of {limit}")]
    OversizedFrame {
        /// Observed frame size in bytes.
        size: usize,
        /// Configured frame cap in bytes.
        limit: usize,
    },
    /// Peer used a protocol version this binary does not speak.
    #[error("unsupported protocol version: expected {expected}, got {got}")]
    UnsupportedVersion {
        /// Version expected by this binary.
        expected: u32,
        /// Version observed on the wire.
        got: u32,
    },
    /// Peer sent a well-formed frame whose kind is illegal in the current state.
    #[error("unexpected frame in {context}: expected {expected}, got {got}")]
    UnexpectedFrame {
        /// State or request that was receiving the frame.
        context: &'static str,
        /// Expected frame kind or set.
        expected: &'static str,
        /// Actual frame kind.
        got: String,
    },
    /// Peer sent a response frame for a different active request.
    #[error("request_id mismatch in {context}: expected {expected}, got {got:?}")]
    RequestIdMismatch {
        /// State or request that was receiving the frame.
        context: &'static str,
        /// Request id currently active on this channel.
        expected: String,
        /// Request id observed on the peer frame.
        got: Option<String>,
    },
    /// Peer disconnected before the request produced its required terminal frame.
    #[error("disconnect before terminal frame in {context}: {cause}")]
    DisconnectBeforeTerminal {
        /// State or request that was awaiting a terminal frame.
        context: &'static str,
        /// Host-visible cause classification at the observation point.
        cause: DisconnectCause,
    },
    /// Peer stopped making read progress before the required terminal frame.
    #[error("read timeout before terminal frame in {context}")]
    ReadTimeout {
        /// State or request that was awaiting a terminal frame.
        context: &'static str,
    },
    /// Peer sent a stream chunk out of sequence.
    #[error("stream sequence mismatch in {stream}: expected {expected}, got {got}")]
    SequenceMismatch {
        /// Stream whose sequence was violated.
        stream: &'static str,
        /// Expected sequence number.
        expected: u64,
        /// Actual sequence number.
        got: u64,
    },
    /// Peer reported a typed failure for a request that should only fail by
    /// returning a terminal error payload.
    #[error("peer rejected {context}: {detail}")]
    PeerRejected {
        /// Request or protocol path that was rejected.
        context: &'static str,
        /// Peer-supplied rejection detail.
        detail: String,
    },
    /// Host-side sequence counter overflowed before the peer sent a terminal
    /// frame. This is a protocol failure because the stream cannot continue
    /// with a unique next sequence number.
    #[error("stream sequence overflow in {stream}")]
    SequenceOverflow {
        /// Stream whose local sequence counter overflowed.
        stream: &'static str,
    },
    /// Peer omitted a required field from a success response.
    #[error("missing required field {field} in {context}")]
    MissingField {
        /// Response context.
        context: &'static str,
        /// Missing field name.
        field: &'static str,
    },
}

/// Finite network-helper operation names used in typed diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkHelperOperation {
    /// Realize bridge/veth/TAP topology for one outbound VM.
    RealizeBridgeAndTap,
    /// Apply outbound NAT sysctl/iptables policy after guest tokens are ready.
    ApplyOutboundNatPolicy,
    /// Clean one VM's outbound network residue.
    CleanupVm,
    /// Clean an orphan run-root bridge.
    CleanupOrphanBridge,
}

impl std::fmt::Display for NetworkHelperOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::RealizeBridgeAndTap => "realize_bridge_and_tap",
            Self::ApplyOutboundNatPolicy => "apply_outbound_nat_policy",
            Self::CleanupVm => "cleanup_vm",
            Self::CleanupOrphanBridge => "cleanup_orphan_bridge",
        })
    }
}

/// Failure while talking to the privileged m80 network helper.
#[derive(Debug, thiserror::Error)]
pub enum NetworkHelperError {
    /// Helper process could not be spawned.
    #[error("network helper {} spawn failed: {source}", path.display())]
    Spawn {
        /// Helper executable path from preflight discovery.
        path: PathBuf,
        /// Spawn failure.
        #[source]
        source: io::Error,
    },
    /// Spawned helper did not expose a required stdio pipe.
    #[error("network helper {} missing {pipe} pipe", path.display())]
    MissingPipe {
        /// Helper executable path from preflight discovery.
        path: PathBuf,
        /// Pipe name.
        pipe: &'static str,
    },
    /// Request serialization failed before the helper saw it.
    #[error("network helper {operation} request encode failed: {detail}")]
    RequestEncode {
        /// Operation being encoded.
        operation: NetworkHelperOperation,
        /// Encoder diagnostic.
        detail: String,
    },
    /// I/O failed while sending or receiving a helper frame.
    #[error("network helper {operation} I/O failed: {source}")]
    Io {
        /// Operation in progress.
        operation: NetworkHelperOperation,
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// Helper response frame exceeded the host-side cap.
    #[error("network helper {operation} response exceeds {limit} bytes")]
    OversizedResponse {
        /// Operation in progress.
        operation: NetworkHelperOperation,
        /// Configured response cap.
        limit: usize,
    },
    /// Helper closed stdout before returning one response frame.
    #[error("network helper {operation} closed stdout before response")]
    Eof {
        /// Operation in progress.
        operation: NetworkHelperOperation,
    },
    /// Response could not be decoded as a finite helper response.
    #[error("network helper {operation} response decode failed: {source}")]
    ResponseDecode {
        /// Operation in progress.
        operation: NetworkHelperOperation,
        /// JSON decode failure.
        #[source]
        source: serde_json::Error,
    },
    /// Helper returned a typed operation failure.
    #[error("network helper {operation} failed ({kind:?}): {detail}")]
    OperationFailed {
        /// Operation that failed.
        operation: NetworkHelperOperation,
        /// Helper failure class.
        kind: NetworkHelperFailureKind,
        /// Helper diagnostic detail.
        detail: String,
    },
    /// Helper returned a success payload that does not match the request.
    #[error("network helper {operation} returned unexpected success kind")]
    UnexpectedSuccess {
        /// Operation in progress.
        operation: NetworkHelperOperation,
    },
    /// Backend construction attempted to switch helper executable paths after
    /// the process-global helper was already started.
    #[error(
        "network helper path mismatch: active {}, requested {}",
        active.display(),
        requested.display()
    )]
    PathMismatch {
        /// Helper path already active for this process.
        active: PathBuf,
        /// Helper path requested by this backend.
        requested: PathBuf,
    },
}

/// Failure while dropping `CAP_NET_ADMIN` from the backend thread.
#[derive(Debug, thiserror::Error)]
pub enum CapabilityDropError {
    /// Reading a Linux capability set failed.
    #[error("failed to read {set} capabilities: {source}")]
    ReadCaps {
        /// Capability set name.
        set: &'static str,
        /// Capability subsystem error.
        #[source]
        source: caps::errors::CapsError,
    },
    /// Writing a Linux capability set failed.
    #[error("failed to update {set} capabilities: {source}")]
    SetCaps {
        /// Capability set name.
        set: &'static str,
        /// Capability subsystem error.
        #[source]
        source: caps::errors::CapsError,
    },
    /// Dropping from the bounding set failed.
    #[error("failed to drop CAP_NET_ADMIN from bounding capabilities: {source}")]
    DropBounding {
        /// Capability subsystem error.
        #[source]
        source: caps::errors::CapsError,
    },
    /// `/proc/thread-self/status` could not be read for post-drop verification.
    #[error("failed to read /proc/thread-self/status after CAP_NET_ADMIN drop: {source}")]
    StatusRead {
        /// I/O failure.
        #[source]
        source: io::Error,
    },
    /// A capability line in `/proc/thread-self/status` was malformed.
    #[error("failed to parse {field} from /proc/thread-self/status: {value}")]
    StatusParse {
        /// Status field name.
        field: &'static str,
        /// Raw field value.
        value: String,
    },
    /// Post-drop verification still observed `CAP_NET_ADMIN`.
    #[error("CAP_NET_ADMIN still present in {field} after backend-thread drop")]
    Verification {
        /// Status field name.
        field: &'static str,
    },
}

/// Bounded lifecycle failure vocabulary used in behavior docs and tests.
///
/// The top-level [`FcError`] variants still preserve the concrete source
/// error. This enum gives operators and future machine readers a stable
/// vocabulary for lifecycle cleanup/readiness classes without reintroducing
/// predecessor agent semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleFailureKind {
    /// Guest daemon did not become ready in the launch/restore budget.
    GuestdNotReady,
    /// Vsock transport failed after readiness should have been established.
    BrokenVsock,
    /// The VM did not respond to a lifecycle operation within its budget.
    StuckVm,
    /// Guest graceful-stop RPC did not complete before its deadline.
    GracefulStopTimeout,
    /// Host forced a Firecracker kill after a graceful path failed or was skipped.
    ForcedKillFallback,
    /// Host-side resource cleanup failed.
    CleanupFailure,
    /// Workspace writeback was intentionally skipped after an unclean stop.
    WritebackSkippedAfterUncleanStop,
}

impl LifecycleFailureKind {
    /// Stable complete list. Adding a variant is a public behavior change.
    pub const ALL: [Self; 7] = [
        Self::GuestdNotReady,
        Self::BrokenVsock,
        Self::StuckVm,
        Self::GracefulStopTimeout,
        Self::ForcedKillFallback,
        Self::CleanupFailure,
        Self::WritebackSkippedAfterUncleanStop,
    ];
}

/// Coarse recovery class for [`FcError`].
///
/// This is intentionally smaller than the error enum. Callers can branch on
/// the broad recovery decision without re-encoding every concrete variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FcErrorKind {
    /// Caller or operator supplied invalid configuration, paths, artifacts, or
    /// used a handle in a way m80 rejects.
    UserInput,
    /// Capacity was unavailable right now; retrying later may succeed without
    /// changing the request.
    ResourceExhaustion,
    /// Host, VMM, transport, or timeout failure that may succeed on a fresh
    /// attempt.
    Transient,
    /// m80 host-side mechanics failed in a way the caller cannot repair by
    /// changing request fields.
    Internal,
    /// Guest or guest-facing request outcome; the host machinery reached the
    /// guest boundary and received or inferred a terminal result.
    GuestOutcome,
}

/// Structured configuration error. Used as the inner payload of
/// [`FcError::Config`].
///
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A TOML config file could not be parsed.
    #[error("{layer} {}: {source}", path.display())]
    TomlSyntax {
        /// Config/profile layer that supplied the TOML.
        layer: &'static str,
        /// Path of the file that failed to parse.
        path: PathBuf,
        /// The parse error from `toml`.
        #[source]
        source: toml::de::Error,
    },
    /// A required configuration field was absent.
    #[error("missing required field `{field}`")]
    MissingField {
        /// Name of the missing field.
        field: &'static str,
    },
    /// A field was present but its value was rejected.
    #[error("invalid value for `{field}`: {reason}")]
    InvalidValue {
        /// Name of the invalid field.
        field: &'static str,
        /// Human-readable rejection reason.
        reason: String,
    },
    /// A content digest failed typed validation.
    #[error("invalid image digest: {reason}")]
    DigestInvalid {
        /// Finite rejection reason.
        reason: &'static str,
    },
    /// A guest mount path failed typed validation.
    #[error("invalid guest mount path: {reason}")]
    MountPathInvalid {
        /// Finite rejection reason.
        reason: &'static str,
    },
    /// A guest mount path shadows a reserved guest root.
    #[error("guest mount path {} shadows a reserved mount point", path.display())]
    MountPathShadowsReserved {
        /// Rejected guest mount path.
        path: PathBuf,
    },
    /// Two pmem layers requested the same guest mount path.
    #[error("duplicate pmem mount path {}", path.display())]
    MountPathDuplicated {
        /// Duplicated guest mount path.
        path: PathBuf,
    },
    /// Too many pmem layers were requested.
    #[error("too many pmem layers: got {got}, max {max}")]
    TooManyLayers {
        /// Maximum accepted pmem layer count.
        max: usize,
        /// Caller-supplied layer count.
        got: usize,
    },
    /// A Shared pmem erofs artifact contains compressed files, which cannot
    /// provide the file-level DAX behavior Shared is admitted for.
    #[error(
        "shared pmem erofs image {} contains {compressed_files} compressed regular files; Shared requires uncompressed erofs payloads",
        path.display()
    )]
    SharedPmemCompressedErofs {
        /// Rejected erofs artifact path.
        path: PathBuf,
        /// Compressed regular-file count reported by `dump.erofs -S`.
        compressed_files: u64,
    },
    /// The host erofs probe produced output m80 cannot classify.
    #[error("could not determine shared pmem erofs layout for {}: {reason}", path.display())]
    SharedPmemErofsLayoutProbeInvalid {
        /// Erofs artifact path being probed.
        path: PathBuf,
        /// Finite parser rejection reason.
        reason: &'static str,
    },
    /// A pmem erofs artifact is too large for Firecracker's v1.15.1
    /// virtio-pmem guest-physical address window.
    #[error(
        "pmem erofs image {} is too large for Firecracker virtio-pmem: got {got} bytes, max {max} bytes",
        path.display()
    )]
    PmemImageTooLarge {
        /// Rejected erofs artifact path.
        path: PathBuf,
        /// Artifact byte length from the image store.
        got: u64,
        /// Maximum accepted artifact byte length.
        max: u64,
    },
    /// The caller-supplied `vm_id` would produce an AF_UNIX socket path that
    /// exceeds the kernel's `sun_path` cap. Surfaces at admission time so the
    /// failure cannot reach `bind()` / `connect()` as an opaque IO error.
    #[error(
        "vm_id {vm_id:?} would produce a {path_len}-byte AF_UNIX socket path under run_root {} (cap {budget})",
        run_root.display()
    )]
    VmIdPathBudgetExceeded {
        /// Caller-supplied vm_id whose path would overflow.
        vm_id: String,
        /// Configured run-root.
        run_root: PathBuf,
        /// Firecracker binary basename consumed by the jail layout.
        fc_basename: String,
        /// Computed path length in bytes.
        path_len: usize,
        /// Usable AF_UNIX `sun_path` cap (kernel reserves 108 bytes including
        /// the null terminator; usable bytes therefore max at 107).
        budget: usize,
    },
}

/// Top-level error for `m80-firecracker`. Each variant tells the caller
/// which phase failed; the inner cause carries phase-specific detail.
#[derive(Debug, thiserror::Error)]
pub enum FcError {
    /// Preflight check failed.
    #[error("preflight: {0}")]
    Preflight(#[from] PreflightError),
    /// Manifest read/validate failed.
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),
    /// Storage operation failed.
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    /// Image-store lookup or verification failed.
    #[error("image store: {0}")]
    ImageStore(#[from] m80_image_store::StoreError),
    /// Jailer materialization or recovery failed.
    #[error("jailer: {0}")]
    Jailer(#[from] JailerError),
    /// Cgroup subtree create / apply_limits / cleanup failed. Distinct
    /// from `Config` so callers can branch on "kernel/cgroup setup
    /// failed" vs "host config didn't parse".
    #[error("cgroup: {0}")]
    Cgroup(#[from] CgroupError),
    /// Network realization or cleanup failed.
    #[error("network: {0}")]
    Network(#[from] NetError),
    /// Privileged network-helper protocol or operation failed.
    #[error("network helper: {0}")]
    NetworkHelper(#[from] NetworkHelperError),
    /// Parent process capability hardening failed.
    #[error("capability drop: {0}")]
    CapabilityDrop(#[from] CapabilityDropError),
    /// Firecracker REST API call failed.
    #[error("client: {0}")]
    Client(#[from] ClientError),
    /// Vsock channel operation failed. Wire-protocol errors arrive here as
    /// `VsockError::Proto(...)` since vsock is the only transport that
    /// speaks `m80-proto` envelopes in this crate; there is no separate
    /// `Proto` variant.
    #[error("vsock: {0}")]
    Vsock(#[from] VsockError),
    /// Host-side protocol failure after a vsock channel was established.
    #[error("protocol: {0}")]
    Protocol(WireProtocolError),
    /// Guestd did not produce a terminal exec frame within the host-enforced
    /// call-wide budget.
    #[error("host exec deadline expired after {timeout:?}")]
    ExecTimeoutHost {
        /// Call-wide host budget that expired.
        timeout: Duration,
    },
    /// Caller-supplied VM id is not a safe single path component.
    #[error("invalid vm_id {vm_id:?}: {reason}")]
    InvalidVmId {
        /// Rejected caller-supplied vm_id.
        vm_id: String,
        /// Human-readable rejection reason.
        reason: String,
    },
    /// Snapshot capture or restore failed.
    #[error("snapshot: {0}")]
    Snapshot(#[from] SnapshotError),
    /// Snapshot-template store lookup, validation, or commit failed.
    #[error("snapshot template: {0}")]
    TemplateStore(#[from] TemplateStoreError),
    /// Guest-side file operation failed with a typed wire error.
    #[error("file operation: {0:?}")]
    FileOp(FileError),
    /// Caller-provided upload reader failed while m80 was streaming chunks to
    /// the guest.
    #[error("file upload source read failed: {source}")]
    FileUploadReadFailed {
        /// Underlying reader error.
        #[source]
        source: io::Error,
    },
    /// Host-side I/O failed where the path is not the useful diagnostic.
    #[error("host I/O during {operation}: {source}")]
    HostIo {
        /// Operation in progress.
        operation: &'static str,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// Guest-side drive hotplug failed with a typed wire error.
    #[error("drive hotplug: {0:?}")]
    DriveHotplug(DriveHotplugError),
    /// Guest-side pmem layer mount failed with a typed wire error.
    #[error("pmem mount: {0:?}")]
    PmemMount(PmemMountError),
    /// Guest-side post-restore hook execution failed with a typed wire error.
    #[error("post-restore hook: {0:?}")]
    PostRestoreHook(HookError),
    /// The recorded Firecracker process was already gone before m80 sent a new
    /// request to a running sandbox.
    #[error("sandbox {vm_id} firecracker pid {firecracker_pid} is not live")]
    SandboxDead {
        /// VM identifier.
        vm_id: String,
        /// Recorded Firecracker process id.
        firecracker_pid: u32,
    },
    /// Guest-mounted drive identity bytes did not match the caller's expected
    /// opaque identity.
    #[error(
        "tenant identity mismatch for {drive_id}: expected {expected_len} bytes, got {actual_len}"
    )]
    TenantIdentityMismatch {
        /// Firecracker drive id whose mounted identity was checked.
        drive_id: String,
        /// Expected opaque identity byte length.
        expected_len: usize,
        /// Actual opaque identity byte length.
        actual_len: usize,
    },
    /// Admission was refused (semaphore at limit; no permit available).
    #[error("admission refused: {limit} concurrent VMs already running")]
    AdmissionRefused {
        /// Configured admission limit.
        limit: u32,
    },
    /// A warm pool allocation was requested while no ready slots were
    /// available. The allocator never hides this with a cold-boot fallback.
    #[error("warm pool empty: 0 ready slots available for target {target_ready}")]
    PoolEmpty {
        /// Configured ready-slot target.
        target_ready: usize,
    },
    /// The lifecycle state machine was in an unexpected state.
    #[error("invalid lifecycle state: expected {expected}, got {actual}")]
    InvalidState {
        /// State the operation expected.
        expected: &'static str,
        /// State the sandbox was actually in.
        actual: &'static str,
    },
    /// Firecracker did not expose its REST API socket within the launch
    /// budget.
    #[error(
        "Firecracker API socket {} did not appear within {timeout:?}",
        path.display()
    )]
    ApiSocketTimeout {
        /// Host path to the API socket m80 waited for.
        path: PathBuf,
        /// Launch budget that expired.
        timeout: Duration,
    },
    /// m80-guestd did not connect on the inverted-readiness socket within
    /// the launch/restore budget.
    #[error(
        "guestd ready signal {} did not arrive within {timeout:?}",
        path.display()
    )]
    GuestdReadyTimeout {
        /// Host-side readiness or vsock path m80 waited on.
        path: PathBuf,
        /// Readiness budget that expired.
        timeout: Duration,
    },
    /// A run directory has an ownership marker that cannot be parsed.
    #[error("run-dir {} has an ambiguous ownership lock", run_dir.display())]
    RunDirOwnershipAmbiguous {
        /// Run directory with the ambiguous marker.
        run_dir: PathBuf,
    },
    /// A run directory is already owned by another live host process.
    #[error("run-dir {} already owned by pid {pid}", run_dir.display())]
    RunDirAlreadyOwned {
        /// Run directory already owned.
        run_dir: PathBuf,
        /// Live owner pid from the lock file.
        pid: u32,
    },
    /// A caller requested a run directory that does not exist.
    #[error("no run-dir found for vm_id={vm_id} at {}", run_dir.display())]
    RunDirNotFound {
        /// Caller-supplied VM id.
        vm_id: String,
        /// Expected run directory path.
        run_dir: PathBuf,
    },
    /// A filesystem operation failed and the path is known at the call site.
    #[error("i/o on {}: {source}", path.display())]
    PathIo {
        /// Path the operation targeted.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// JSON serialization failed before an operation could write its target.
    #[error("json in {context}: {source}")]
    Json {
        /// Operation that was serializing JSON.
        context: &'static str,
        /// Underlying serde error.
        #[source]
        source: serde_json::Error,
    },
    /// A public API exists in the v0.x surface but is intentionally unavailable.
    #[error("{operation} is unsupported: {reason}")]
    UnsupportedOperation {
        /// API or command that was requested.
        operation: &'static str,
        /// Why the operation is unavailable.
        reason: String,
    },
    /// Spawning a host helper command failed before an exit status existed.
    #[error("failed to spawn command `{command}`: {source}")]
    CommandSpawnFailed {
        /// Host command label.
        command: &'static str,
        /// Spawn failure.
        #[source]
        source: io::Error,
    },
    /// A host helper command exited unsuccessfully.
    #[error("command `{command}` failed with {status}{output}")]
    CommandFailed {
        /// Host command label.
        command: &'static str,
        /// Process exit status.
        status: ExitStatus,
        /// Captured output, prefixed with `: ` when present.
        output: String,
    },
    /// A required artifact was absent after unpacking or installation.
    #[error("required artifact missing: {}", path.display())]
    ArtifactMissing {
        /// Missing artifact path.
        path: PathBuf,
    },
    /// Warm-pool background fill failed before the requested ready count arrived.
    #[error("warm pool fill failed: {detail}")]
    WarmPoolFillFailed {
        /// Most recent fill error detail recorded by the background worker.
        detail: String,
    },
    /// Warm-pool ready probe produced a non-ready exec response.
    #[error("warm pool ready probe failed: status={status:?} exit_code={exit_code:?}")]
    WarmReadyProbeRejected {
        /// Exec terminal status.
        status: ExecStatus,
        /// Guest exit code, when one was present.
        exit_code: Option<i32>,
    },
    /// A warm-owner socket already exists where a new owner would bind.
    #[error("warm owner socket already exists at {}; run `m80 warm disable` first", socket_path.display())]
    WarmOwnerSocketExists {
        /// Existing socket path.
        socket_path: PathBuf,
    },
    /// Warm owner is draining and will not accept new leases.
    #[error("warm owner is draining and not accepting leases")]
    WarmOwnerNotAcceptingLeases,
    /// Warm owner drain did not settle in the caller's deadline.
    #[error("warm owner drain timed out after {timeout:?} waiting for filling slots")]
    WarmOwnerDrainTimeout {
        /// Drain settle budget.
        timeout: Duration,
    },
    /// A warm run request targets a different owner profile or egress policy.
    #[error("warm {field} mismatch: requested {requested}, owner active {active}")]
    WarmCompatibilityMismatch {
        /// Compatibility dimension that differed.
        field: &'static str,
        /// Caller-requested value.
        requested: String,
        /// Owner-active value.
        active: String,
    },
    /// The warm owner returned a valid envelope of the wrong response kind.
    #[error("warm owner returned {response} for {request} request")]
    UnexpectedWarmResponse {
        /// Request kind the caller sent.
        request: &'static str,
        /// Response kind received.
        response: &'static str,
    },
    /// SIGKILL failed for a concrete host pid.
    #[error("failed to kill pid {pid}: {source}")]
    KillFailed {
        /// Host pid that was targeted.
        pid: u32,
        /// Underlying syscall error.
        #[source]
        source: io::Error,
    },
    /// Host timed out waiting for a killed child process to reap.
    #[error("timed out reaping pid {pid} after SIGKILL within {timeout:?}")]
    ReapTimeout {
        /// Host pid that did not reap in time.
        pid: u32,
        /// Reap budget.
        timeout: Duration,
    },
    /// waitpid failed for a concrete host pid.
    #[error("failed to reap pid {pid}: {source}")]
    ReapFailed {
        /// Host pid passed to waitpid.
        pid: u32,
        /// Underlying syscall error.
        #[source]
        source: io::Error,
    },
    /// Configuration loading or merging failure.
    #[error("config: {0}")]
    Config(ConfigError),
    /// The sandbox was idle for longer than `SandboxConfig::idle_timeout`.
    ///
    /// The background watcher has issued a graceful shutdown; the caller must
    /// not send further exec requests. Drop or `stop()` the sandbox to release
    /// resources.
    #[error("sandbox idle timeout expired")]
    IdleTimedOut,
    /// The sandbox is configured for one workload and that workload already
    /// began. The caller must stop/drop/discard this VM instead of reusing it.
    #[error("one-shot sandbox already consumed")]
    OneShotConsumed,
}

impl FcError {
    /// Stable variant name without payload detail.
    #[must_use]
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::Preflight(_) => "Preflight",
            Self::Manifest(_) => "Manifest",
            Self::Storage(_) => "Storage",
            Self::ImageStore(_) => "ImageStore",
            Self::Jailer(_) => "Jailer",
            Self::Cgroup(_) => "Cgroup",
            Self::Network(_) => "Network",
            Self::NetworkHelper(_) => "NetworkHelper",
            Self::CapabilityDrop(_) => "CapabilityDrop",
            Self::Client(_) => "Client",
            Self::Vsock(_) => "Vsock",
            Self::Protocol(_) => "Protocol",
            Self::ExecTimeoutHost { .. } => "ExecTimeoutHost",
            Self::InvalidVmId { .. } => "InvalidVmId",
            Self::Snapshot(_) => "Snapshot",
            Self::TemplateStore(_) => "TemplateStore",
            Self::FileOp(_) => "FileOp",
            Self::FileUploadReadFailed { .. } => "FileUploadReadFailed",
            Self::HostIo { .. } => "HostIo",
            Self::DriveHotplug(_) => "DriveHotplug",
            Self::PmemMount(_) => "PmemMount",
            Self::PostRestoreHook(_) => "PostRestoreHook",
            Self::SandboxDead { .. } => "SandboxDead",
            Self::TenantIdentityMismatch { .. } => "TenantIdentityMismatch",
            Self::AdmissionRefused { .. } => "AdmissionRefused",
            Self::PoolEmpty { .. } => "PoolEmpty",
            Self::InvalidState { .. } => "InvalidState",
            Self::ApiSocketTimeout { .. } => "ApiSocketTimeout",
            Self::GuestdReadyTimeout { .. } => "GuestdReadyTimeout",
            Self::RunDirOwnershipAmbiguous { .. } => "RunDirOwnershipAmbiguous",
            Self::RunDirAlreadyOwned { .. } => "RunDirAlreadyOwned",
            Self::RunDirNotFound { .. } => "RunDirNotFound",
            Self::PathIo { .. } => "PathIo",
            Self::Json { .. } => "Json",
            Self::UnsupportedOperation { .. } => "UnsupportedOperation",
            Self::CommandSpawnFailed { .. } => "CommandSpawnFailed",
            Self::CommandFailed { .. } => "CommandFailed",
            Self::ArtifactMissing { .. } => "ArtifactMissing",
            Self::WarmPoolFillFailed { .. } => "WarmPoolFillFailed",
            Self::WarmReadyProbeRejected { .. } => "WarmReadyProbeRejected",
            Self::WarmOwnerSocketExists { .. } => "WarmOwnerSocketExists",
            Self::WarmOwnerNotAcceptingLeases => "WarmOwnerNotAcceptingLeases",
            Self::WarmOwnerDrainTimeout { .. } => "WarmOwnerDrainTimeout",
            Self::WarmCompatibilityMismatch { .. } => "WarmCompatibilityMismatch",
            Self::UnexpectedWarmResponse { .. } => "UnexpectedWarmResponse",
            Self::KillFailed { .. } => "KillFailed",
            Self::ReapTimeout { .. } => "ReapTimeout",
            Self::ReapFailed { .. } => "ReapFailed",
            Self::Config(_) => "Config",
            Self::IdleTimedOut => "IdleTimedOut",
            Self::OneShotConsumed => "OneShotConsumed",
        }
    }

    /// Coarse recovery class for caller policy.
    #[must_use]
    pub fn kind(&self) -> FcErrorKind {
        match self {
            Self::Preflight(_)
            | Self::Manifest(_)
            | Self::InvalidVmId { .. }
            | Self::RunDirNotFound { .. }
            | Self::UnsupportedOperation { .. }
            | Self::ArtifactMissing { .. }
            | Self::Config(_)
            | Self::FileUploadReadFailed { .. }
            | Self::WarmOwnerSocketExists { .. }
            | Self::WarmCompatibilityMismatch { .. }
            | Self::OneShotConsumed => FcErrorKind::UserInput,

            Self::AdmissionRefused { .. }
            | Self::PoolEmpty { .. }
            | Self::RunDirAlreadyOwned { .. }
            | Self::WarmOwnerNotAcceptingLeases
            | Self::WarmOwnerDrainTimeout { .. } => FcErrorKind::ResourceExhaustion,

            Self::Client(_)
            | Self::Vsock(_)
            | Self::Protocol(_)
            | Self::ExecTimeoutHost { .. }
            | Self::SandboxDead { .. }
            | Self::ApiSocketTimeout { .. }
            | Self::GuestdReadyTimeout { .. }
            | Self::HostIo { .. }
            | Self::WarmPoolFillFailed { .. }
            | Self::KillFailed { .. }
            | Self::ReapTimeout { .. }
            | Self::ReapFailed { .. } => FcErrorKind::Transient,

            Self::FileOp(_)
            | Self::DriveHotplug(_)
            | Self::PmemMount(_)
            | Self::PostRestoreHook(_)
            | Self::TenantIdentityMismatch { .. }
            | Self::WarmReadyProbeRejected { .. }
            | Self::IdleTimedOut => FcErrorKind::GuestOutcome,

            Self::Storage(_)
            | Self::ImageStore(_)
            | Self::Jailer(_)
            | Self::Cgroup(_)
            | Self::Network(_)
            | Self::NetworkHelper(_)
            | Self::CapabilityDrop(_)
            | Self::Snapshot(_)
            | Self::TemplateStore(_)
            | Self::InvalidState { .. }
            | Self::RunDirOwnershipAmbiguous { .. }
            | Self::PathIo { .. }
            | Self::Json { .. }
            | Self::CommandSpawnFailed { .. }
            | Self::CommandFailed { .. }
            | Self::UnexpectedWarmResponse { .. } => FcErrorKind::Internal,
        }
    }

    /// Whether the same logical request may reasonably be retried later or on
    /// a fresh sandbox without changing caller input.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.kind(),
            FcErrorKind::ResourceExhaustion | FcErrorKind::Transient
        )
    }

    /// Whether the error points at caller/operator input rather than a
    /// transient or host-internal failure.
    #[must_use]
    pub fn is_user_error(&self) -> bool {
        self.kind() == FcErrorKind::UserInput
    }
}

/// Ordered host-side teardown phases for a running VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupPhase {
    /// Stop accepting new work on this handle.
    AdmissionFence,
    /// Bound the guest shutdown attempt and host kill step.
    BoundedStop,
    /// Optional caller-requested scratch extraction after stop.
    OptionalChangeExtract,
    /// Drop or clean owned host resources.
    ResidueCleanup,
    /// Release the admission permit by deleting or preserving the stopped run-dir.
    Release,
}

/// Current v0.1 teardown phase order.
pub const CLEANUP_PHASE_ORDER: [CleanupPhase; 5] = [
    CleanupPhase::AdmissionFence,
    CleanupPhase::BoundedStop,
    CleanupPhase::OptionalChangeExtract,
    CleanupPhase::ResidueCleanup,
    CleanupPhase::Release,
];

/// Observable stop paths exposed by `m80-firecracker`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopDisposition {
    /// Ask m80-guestd to shut down, then kill the Firecracker pid.
    GuestdShutdownThenFirecrackerKill,
    /// Skip the guest request and kill Firecracker plus jailer pids.
    HostForceKill,
}

/// Current v0.1 stop dispositions.
pub const STOP_DISPOSITIONS: [StopDisposition; 2] = [
    StopDisposition::GuestdShutdownThenFirecrackerKill,
    StopDisposition::HostForceKill,
];

/// Generic conditions that prevent a caller from treating cleanup as releasable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupReleaseBlocker {
    /// The host cannot prove the forced kill completed cleanly.
    ForcedKillAmbiguous,
    /// Owned host cleanup returned an error.
    CleanupFailed,
    /// Owned residue may still represent a live VM.
    OwnedResidueMayStillBeLive,
}

/// Generic release blockers owned by m80's VM mechanics.
pub const CLEANUP_RELEASE_BLOCKERS: [CleanupReleaseBlocker; 3] = [
    CleanupReleaseBlocker::ForcedKillAmbiguous,
    CleanupReleaseBlocker::CleanupFailed,
    CleanupReleaseBlocker::OwnedResidueMayStillBeLive,
];

/// Authority boundary for cleanup decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupAuthority {
    /// m80 emits local lifecycle evidence but does not advance placement state.
    EvidenceOnly,
}

/// m80-firecracker's cleanup authority mode.
pub const CLEANUP_AUTHORITY: CleanupAuthority = CleanupAuthority::EvidenceOnly;
