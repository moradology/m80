# UDS REST API client behaviors

Behaviors captured in `m80-firecracker-client`. All present-tense facts
extracted from predecessor `crates/sandbox/agent-sandbox-firecracker/src/client.rs`.

## sync-http-over-uds

The client performs Firecracker REST calls synchronously over a `UnixStream`
connection to the API socket without an async runtime. Each method call opens
the stream (or reuses the stored one), writes a raw HTTP/1.1 PUT request, reads
the response in a single blocking loop, and returns before the next call can
proceed.

predecessor source: `client.rs:2,291` (`use std::os::unix::net::UnixStream`,
`UnixStream::connect`).

Test: `crates/m80-firecracker-client/tests/put_each_resource.rs` — every
resource PUT goes through the fixture server on a `UnixStream`; the test
blocks until the response is received, confirming the blocking contract.

## typed-configs

The client carries `BootSourceConfig`, `MachineConfig`, `DriveConfig`,
`NetworkInterfaceConfig`, and `VsockConfig` as typed Rust structs that
serialize to the Firecracker REST schema via `serde_json`. Optional fields are
omitted with `skip_serializing_if = "Option::is_none"` to match the schema
exactly.

predecessor source: `client.rs:134-159` (`put_machine_config`, `put_boot_source`,
`put_drive`, `put_vsock`, `put_network_interface`).

Test: `crates/m80-firecracker-client/tests/put_each_resource.rs` — each
resource PUT records the request body; assertions check JSON field names and
values.

## instance-start

The client issues `{"action_type":"InstanceStart"}` via PUT `/actions` to begin
VM execution. The `InstanceAction::InstanceStart` variant serializes to
`"InstanceStart"` via `#[serde(rename_all = "PascalCase")]`.

predecessor source: `client.rs:91-99,163-164` (`InstanceActionType::InstanceStart`).

Test: `crates/m80-firecracker-client/tests/instance_action_serialization.rs::instance_start_serializes_to_pascal_case`.

## send-ctrl-alt-del

The client issues `{"action_type":"SendCtrlAltDel"}` via PUT `/actions` to
request a graceful guest shutdown. The client itself does NOT perform an
architecture check; that gate lives in the orchestrator (`m80-firecracker`).
The contract (documented in the README) is that `SendCtrlAltDel` is passed
through verbatim, and Firecracker itself only honors it on x86_64.

predecessor source: `client.rs:99-100,173-174` (`InstanceActionType::SendCtrlAltDel`);
arch-aware dispatch is in `lifecycle.rs:1286,1297-1305`.

Test: `crates/m80-firecracker-client/tests/instance_action_serialization.rs::send_ctrl_alt_del_serializes_to_pascal_case`.

## typed-errors

API failures map to per-resource typed variants: `BootSourceWriteFailed`,
`MachineConfigWriteFailed`, `DriveWriteFailed`, `NetworkInterfaceWriteFailed`,
`VsockWriteFailed`, `InstanceActionFailed`. Each variant carries the raw
Firecracker fault body verbatim in a `fault: String` field. `Connect(io::Error)`
is returned when the UDS cannot be opened; `Io(io::Error)` for transport
failures during a call.

predecessor source: `errors.rs:858-901` (Api* variants; m80 renames and
per-resource-types these to avoid the generic `endpoint: &'static str` field).

Test: `crates/m80-firecracker-client/tests/error_mapping.rs` — fixture server
returns 400 with a JSON fault body for each resource; assertions match the
corresponding `ClientError::*WriteFailed` variant and check the fault string.

## blocking-no-pool

The client serializes Firecracker REST calls per VM through one stored
`UnixStream` (wrapped in `Mutex<UnixStream>` for `&self` method access). There
is no connection pool. Concurrency is the caller's responsibility; the README
explicitly documents that concurrent config writes are not handled cleanly by
Firecracker.

predecessor source: `client.rs:2,291` (`UnixStream::connect` per-call in predecessor;
m80 stores one stream per `Client` but the single-connection, no-pool contract
is the same).

Test: `crates/m80-firecracker-client/tests/put_each_resource.rs` — all six
PUT calls succeed sequentially on the same `Client` instance over one
`UnixStream` connection.
