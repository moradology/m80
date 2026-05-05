# `m80-firecracker-client`

A pure REST speaker for the Firecracker UDS API — `BootSource`, `Drive`,
`MachineConfig`, `Vsock`, `InstanceAction`. No state. No policy. No
knowledge of m80's lifecycle. (`NetworkInterface` lands in v0.2 with
`OutboundNat`.)

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
  `put_boot_source`, `put_machine_config`, `put_drive`, `put_vsock`,
  `instance_action`. The method signature mirrors the Firecracker schema
  exactly.
- The client owns no global state. Constructing one is `Client::new(uds_path)`;
  dropping it closes the underlying socket. Multiple clients can target
  the same UDS, but the caller is responsible for serializing concurrent
  access (Firecracker itself does not handle concurrent config writes
  cleanly).
- HTTP errors translate to typed `ClientError` variants per resource:
  `BootSourceWriteFailed`, `MachineConfigWriteFailed`,
  `DriveWriteFailed`, `VsockWriteFailed`, `InstanceActionFailed`. Each
  carries the Firecracker fault JSON verbatim.
- `instance_action(InstanceAction::SendCtrlAltDel)` is callable on any
  arch but only honored on `x86_64` by Firecracker itself; the client
  passes through the upstream behavior without arch-checking.

## Public surface

- `Client::new(uds_path: &Path) -> Result<Client, ClientError>`.
- One method per Firecracker resource, taking the resource's config
  struct (re-exported from this crate) and returning `Result<(), ClientError>`.
- `InstanceAction { InstanceStart, SendCtrlAltDel, FlushMetrics, Pause, Resume }`.
- Firecracker config types: `BootSourceConfig`, `MachineConfig`,
  `DriveConfig`, `VsockConfig`.
- `ClientError` — typed per-resource failure.

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

## Tests

- `tests/put_each_resource.rs` — for each Firecracker resource, a fixture
  `UnixListener` records the request body and asserts the JSON shape; the
  client returns `Ok(())` on a fixture 204.
- `tests/error_mapping.rs` — fixture server returns a 400 with a
  Firecracker fault body for each resource; asserts the matching typed
  `ClientError::*WriteFailed` variant fires (and `Connect(io::Error)` on
  a missing socket).
- `tests/instance_action_serialization.rs` — every `InstanceAction`
  variant serializes to the expected `{"action_type": "<PascalCase>"}`
  body.

Real-firecracker conformance + concurrency-probe tests are deferred
until we have a CI-managed firecracker binary in fixtures.
