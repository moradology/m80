# `m80-firecracker-client`

A pure REST speaker for the Firecracker UDS API — `BootSource`, `Drive`,
`MachineConfig`, `NetworkInterface`, `Vsock`, `InstanceAction`. No state.
No policy. No knowledge of m80's lifecycle.

## Reason for being

The Firecracker control plane is a small, stable HTTP-over-UDS API. The
predecessor implementation entangles "speak the API" with "know what order to
PUT things in" inside one ~900 LOC `client.rs`. m80 separates them:
`m80-firecracker-client` answers "make this PUT call"; `m80-firecracker`
(the orchestrator) answers "what calls go in what order".

A second motivation: a generic Firecracker REST speaker is reusable beyond
m80. Anyone debugging a Firecracker VM, building a snapshot tool, or
writing a different orchestrator should be able to depend on this crate
without inheriting m80's lifecycle assumptions.

## Black-box contract

- The client speaks **synchronous** HTTP-over-UDS. There is no
  runtime/executor dependency; one call blocks until Firecracker responds.
  Concurrency is the caller's job.
- Every API method maps 1:1 to a Firecracker REST resource:
  `put_boot_source`, `put_machine_config`, `put_drive`,
  `put_network_interface`, `put_vsock`, `put_entropy_device`, `patch_drive`,
  `instance_action`, `patch_vm_state`, `put_snapshot_create`,
  `put_snapshot_load`. The method signature mirrors the Firecracker schema
  exactly.
- The client owns no global state. Constructing one is `Client::new(uds_path)`;
  dropping it closes the underlying socket. Multiple clients can target
  the same UDS, but the caller is responsible for serializing concurrent
  access (Firecracker itself does not handle concurrent config writes
  cleanly).
- HTTP errors translate to typed `ClientError` variants per resource:
  `BootSourceWriteFailed`, `MachineConfigWriteFailed`,
  `DriveWriteFailed`, `NetworkInterfaceWriteFailed`, `VsockWriteFailed`,
  `EntropyDeviceWriteFailed`, `InstanceActionFailed`, `VmStateWriteFailed`,
  `SnapshotCreateFailed`, `SnapshotLoadFailed`.
  Each carries the Firecracker fault JSON verbatim.
- Request serialization errors surface as `ClientError::Serialize`; they are
  not collapsed into an I/O error because no socket operation occurred.
## Public surface

- `Client::new(uds_path: &Path) -> Result<Client, ClientError>`.
- One method per Firecracker resource, taking the resource's config
  struct (re-exported from this crate) and returning `Result<(), ClientError>`.
  `put_entropy_device` has no config argument because the default virtio-rng
  body is an empty object.
- `CpuTemplate { T2, C3 }` — optional CPU template on `MachineConfig`.
- `IoEngine { Sync, Async }` — optional Firecracker block-device I/O engine
  on `DriveConfig`.
- `InstanceAction { InstanceStart }`.
- `VmState { Paused, Resumed }` — for `patch_vm_state`.
- `SnapshotType { Full, Diff }` — for `CreateSnapshotConfig`.
- `MemBackendType { File, Uffd }` — for `MemBackendConfig`.
- Firecracker config types: `BootSourceConfig`, `MachineConfig`,
  `DriveConfig`, `PartialDriveConfig`, `NetworkInterfaceConfig`,
  `VsockConfig`, `CreateSnapshotConfig`, `LoadSnapshotConfig`,
  `MemBackendConfig`, `VsockOverride`.
- `ClientError` — typed per-resource failure plus `Connect`, `Serialize`,
  and socket-path-carrying `Io`.

## Non-goals

- **No lifecycle ordering.** "PUT machine config before boot source" is
  not enforced here; the orchestrator (`m80-firecracker`) decides.
- **No async runtime.** Wrap the calls in `spawn_blocking` if you need
  async; the crate itself stays runtime-agnostic.
- **No retry policy.** Errors propagate immediately.
- **No metrics endpoint scraping.** Reading Firecracker's metrics fifo is
  a separate concern handled in `m80-observability`.

## Dependencies

- `serde`, `serde_json`, `thiserror`, `tracing`.
- A small custom HTTP/1.1 client over `UnixStream` (no `hyper`/`reqwest`
  dependency — m80 wants the smallest possible footprint here).
- (no other m80 crates.)

## Debug instrumentation

Set `M80_DEBUG_WIRE=fcrest` (or `M80_DEBUG_WIRE=all`) to enable wire-level
logging via `tracing::trace!`. When enabled, every Firecracker REST PUT
request (method, path, and body) and response (status and body) is logged
with a hex+ASCII preview of up to 1024 bytes.

- Matching is exact (`==`): `M80_DEBUG_WIRE= fcrest` (leading space) does
  **not** match; `M80_DEBUG_WIRE=fcrest` does.
- Multiple targets are comma-separated: `M80_DEBUG_WIRE=vsock,fcrest`.
- `all` matches every target.
- Unknown tokens are silently ignored.
- The gate is a single atomic load on the hot path; no serialization occurs
  unless the gate fires.

See `crates/m80-firecracker/README.md` "Debug instrumentation" for the
complete table of all recognized `M80_DEBUG_WIRE` targets across the workspace.

## Tests

- `tests/put_each_resource.rs` — for each Firecracker resource, a fixture
  `UnixListener` records the request body and asserts the JSON shape; the
  client returns `Ok(())` on a fixture 204. Drive tests cover preboot
  `PUT /drives/{id}` and post-boot `PATCH /drives/{id}` partial updates.
  Machine-config tests pin the serialized CPU template field. Entropy tests
  pin `PUT /entropy` with `{}`.
- `tests/error_mapping.rs` — fixture server returns a 400 with a
  Firecracker fault body for each resource; asserts the matching typed
  `ClientError::*WriteFailed` variant fires (and `Connect(io::Error)` on
  a missing socket).
- `tests/instance_action_serialization.rs` — `InstanceAction::InstanceStart`
  serializes to the expected `{"action_type": "InstanceStart"}` body.
- `tests/network_interface_config_round_trip.rs` — URL, optional-field
  omission, and typed 400 error mapping for `PUT /network-interfaces/{id}`.
- `tests/snapshot.rs` — fixture-server tests for `patch_vm_state`,
  `put_snapshot_create`, and `put_snapshot_load`: URL, required fields,
  optional-field omission, `resume_vm`, `vsock_override`, and 400 error
  mapping for each method.

Real-firecracker conformance + concurrency-probe tests are deferred
until we have a CI-managed firecracker binary in fixtures.
