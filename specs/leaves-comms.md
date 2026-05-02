# Leaves: Communications stratum (L1-04, L1-05, L1-06, L1-12, L1-14)

This file enumerates leaf beads for the five comms-and-coordination L1 epics.
All leaves carry the `$ACTIVE` label and one behavior-domain tag from
{`guest-exec`, `wire-protocol`, `vsock`, `configuration`, `concurrency`}.

The L1-04 and L1-05 leaves are **REFRAMED** for m80's generic-sandbox scope:
the daemon runs one operation (exec argv with optional cwd/env →
stdout/stderr/exit/timing/status); the wire is opaque request/response with
a version handshake. There are no leaves about tool catalogs,
`tool_call_id`/`correlation_id`/`idempotency_key` semantics,
`EffectClass`, or `WorkspacePolicy` — those are agent-tier and live in a
future `m80-adapter`.

---

# L1-04 Generic Guest Exec Service

## L2-04.1 Image contents and systemd boot (parent_var: $L2_04_1)

### Leaf: Install m80 guest daemon binary at /usr/local/bin in the rootfs
- parent_var: $L2_04_1
- labels: $ACTIVE,guest-exec,image
- status: open
- behavior: The image-build pipeline installs the guest daemon binary at `/usr/local/bin/m80-guestd` inside the rootfs so systemd can launch it on boot.
- source: dossier `03-guest-daemon.md` § Image contents; predecessor `infra/firecracker/prepare-guestd-image.sh` lines 5,205,219-220 (installs `guestd-rs` binary and unit symlink).
- captured-by: m80/docs/behaviors/guest-exec/image-contents.md#daemon-binary + m80/m80-image/tests/image/contents.rs::installs_guest_daemon_binary

### Leaf: Install systemd unit Type=simple Restart=always at boot wants
- parent_var: $L2_04_1
- labels: $ACTIVE,guest-exec,image
- status: open
- behavior: The image installs `/etc/systemd/system/m80-guestd.service` with `Type=simple`, `Restart=always`, `RestartSec=1`, and a `multi-user.target.wants` symlink so the daemon starts on boot.
- source: dossier `03-guest-daemon.md` § Image contents; predecessor `infra/firecracker/guestd-rs.service` lines 7-13 + `prepare-guestd-image.sh:220`.
- captured-by: m80/docs/behaviors/guest-exec/image-contents.md#systemd-unit + m80/m80-image/tests/image/systemd_unit.rs::installs_service_unit_with_restart_always

### Leaf: Install workspace mount unit binding /dev/vdb ext4 at fixed path
- parent_var: $L2_04_1
- labels: $ACTIVE,guest-exec,image
- status: open
- behavior: The image installs a systemd `.mount` unit that mounts `/dev/vdb` (the scratch workspace block device) at the fixed in-guest path before the guest daemon starts.
- source: dossier `03-guest-daemon.md` § Image contents; predecessor `infra/firecracker/var-lib-predecessor-workspace.mount` lines 4-8 (`What=/dev/vdb`, `Type=ext4`) and `guestd-rs.service:5-6` (`Requires=` + `After=` workspace mount).
- captured-by: m80/docs/behaviors/guest-exec/image-contents.md#workspace-mount + m80/m80-image/tests/image/systemd_unit.rs::workspace_mount_orders_before_daemon

### Leaf: Install /etc/default env file consumed by guest daemon unit
- parent_var: $L2_04_1
- labels: $ACTIVE,guest-exec,image
- status: open
- behavior: The image installs an `/etc/default/m80-guestd` environment file that the systemd unit consumes via `EnvironmentFile=-` so host-injected configuration (e.g. ready-marker overrides) reaches the daemon at startup.
- source: dossier `03-guest-daemon.md` § Image contents; predecessor `infra/firecracker/prepare-guestd-image.sh:7` (`GUESTD_ENV_FILE_PATH=/etc/default/guestd-rs`) + `guestd-rs.service:10`.
- captured-by: m80/docs/behaviors/guest-exec/image-contents.md#env-file + m80/m80-image/tests/image/systemd_unit.rs::installs_environment_file

## L2-04.2 Vsock listener and accept loop (parent_var: $L2_04_2)

### Leaf: Bind a vsock listener on a fixed port at startup
- parent_var: $L2_04_2
- labels: $ACTIVE,guest-exec,vsock-listen
- status: open
- behavior: The guest daemon binds a vsock listener on the configured port (default 9001) at startup and emits a ready marker before accepting any connections.
- source: dossier `03-guest-daemon.md` § Main components; predecessor `services/guestd-rs/src/main.rs:280-282` (`bind_vsock_listener` + `emit_vsock_ready_marker`).
- captured-by: m80/docs/behaviors/guest-exec/listener.md#bind + m80/m80-guestd/tests/listener/bind.rs::binds_listener_then_emits_ready

### Leaf: Accept one vsock connection per request and dispatch sequentially
- parent_var: $L2_04_2
- labels: $ACTIVE,guest-exec,vsock-listen
- status: open
- behavior: The accept loop processes vsock connections one at a time: read the request envelope, run the exec, write the response, flush, close, then accept the next connection.
- source: dossier `03-guest-daemon.md` § Main components; predecessor `services/guestd-rs/src/main.rs:318-329` (`serve_vsock_connections` accept loop calling `serve_connection` synchronously).
- captured-by: m80/docs/behaviors/guest-exec/listener.md#accept-loop + m80/m80-guestd/tests/listener/accept_loop.rs::dispatches_one_request_per_connection

### Leaf: Sync filesystems before closing the response stream
- parent_var: $L2_04_2
- labels: $ACTIVE,guest-exec,vsock-listen
- status: open
- behavior: The connection handler flushes the response writer (and, when a workspace is mounted, syncs the workspace filesystem) before closing the connection so the host sees committed bytes.
- source: dossier `03-guest-daemon.md` § What runs inside the VM ("flushes filesystems, closes"); predecessor `services/guestd-rs/src/main.rs:487-490` (`output.write_all` + `output.flush` before connection drop).
- captured-by: m80/docs/behaviors/guest-exec/listener.md#flush-before-close + m80/m80-guestd/tests/listener/flush.rs::flushes_response_before_closing

## L2-04.3 Process orchestration (parent_var: $L2_04_3)

### Leaf: Spawn child with caller-supplied argv plus optional cwd and env
- parent_var: $L2_04_3
- labels: $ACTIVE,guest-exec,orchestration
- status: open
- behavior: The guest daemon spawns the child process directly from the caller-supplied `argv`, applying the optional `cwd` and `env` overrides from the request envelope without consulting any tool catalog.
- source: dossier `02-sandbox-api-and-guest-proto.md` § Resolution (M80Request reframe) + `03-guest-daemon.md` § Process orchestration; predecessor `crates/sandbox/agent-sandbox-local/src/executor.rs` (subprocess spawn + capture).
- captured-by: m80/docs/behaviors/guest-exec/orchestration.md#spawn + m80/m80-guestd/tests/orchestration/spawn.rs::spawns_argv_with_cwd_and_env

### Leaf: Capture stdout and stderr into the response
- parent_var: $L2_04_3
- labels: $ACTIVE,guest-exec,orchestration
- status: open
- behavior: The daemon captures the child's stdout and stderr byte streams and returns them as inline payloads on the response envelope, paired with the exit code and timing metadata.
- source: dossier `02-sandbox-api-and-guest-proto.md` § ExecutionResponse shape (stdout/stderr/exit_code/timing) + `03-guest-daemon.md` § Process orchestration; predecessor `crates/sandbox/agent-sandbox-local/src/executor.rs` capture loop.
- captured-by: m80/docs/behaviors/guest-exec/orchestration.md#capture + m80/m80-guestd/tests/orchestration/capture.rs::returns_stdout_stderr_and_exit_code

### Leaf: Apply caller timeout and report TimedOut on overrun with SIGKILL
- parent_var: $L2_04_3
- labels: $ACTIVE,guest-exec,orchestration
- status: open
- behavior: When the caller-supplied timeout elapses before the child exits the daemon sends SIGKILL, marks the response status as `TimedOut`, and still returns whatever stdout/stderr was buffered before the kill.
- source: dossier `02-sandbox-api-and-guest-proto.md` § ExecutionResponse shape (`TimedOut` status); predecessor `crates/sandbox/agent-sandbox-local/src/executor.rs` (timeout-driven kill path) + `agent-guest-proto/src/envelope.rs` `ExecutionStatus::TimedOut`.
- captured-by: m80/docs/behaviors/guest-exec/orchestration.md#timeout + m80/m80-guestd/tests/orchestration/timeout.rs::reports_timed_out_status_and_kills_child

## L2-04.4 Cancellation (parent_var: $L2_04_4)

### Leaf: Kill the child when the vsock connection drops mid-execution
- parent_var: $L2_04_4
- labels: $ACTIVE,guest-exec,cancellation
- status: open
- behavior: If the host disconnects the vsock connection while a child is running, the daemon detects the broken stream and SIGKILLs the child so resources are not held after the caller is gone.
- source: dossier `03-guest-daemon.md` § What it can do generically (process orchestration); predecessor `services/guestd-rs/src/main.rs:318-329` (per-connection error path drops stream and reaps).
- captured-by: m80/docs/behaviors/guest-exec/cancellation.md#disconnect-kills-child + m80/m80-guestd/tests/cancellation/disconnect.rs::kills_child_when_connection_drops

### Leaf: Flush partial output buffers before the cancelled response closes
- parent_var: $L2_04_4
- labels: $ACTIVE,guest-exec,cancellation
- status: open
- behavior: On cancellation the daemon flushes whatever stdout and stderr bytes were already collected so any in-flight response or log fixture observes the partial output rather than a truncated zero-length payload.
- source: dossier `03-guest-daemon.md` § Process orchestration; predecessor `crates/sandbox/agent-sandbox-local/src/executor.rs` (capture buffers preserved on early termination).
- captured-by: m80/docs/behaviors/guest-exec/cancellation.md#partial-flush + m80/m80-guestd/tests/cancellation/partial_flush.rs::flushes_partial_output_on_cancel

---

# L1-05 Generic Wire Protocol

## L2-05.1 Envelope schema (parent_var: $L2_05_1)

### Leaf: Stamp every envelope with an explicit u32 protocol version field
- parent_var: $L2_05_1
- labels: $ACTIVE,wire-protocol,envelope
- status: open
- behavior: Every request and response envelope carries an explicit `version: u32` field stamped to `PROTOCOL_VERSION` so peers can fail-closed on mismatch without parsing the rest of the payload.
- source: dossier `02-sandbox-api-and-guest-proto.md` § Guest protocol; predecessor `crates/sandbox/agent-guest-proto/src/envelope.rs:42-44,77-79` (`pub version: u32` on `GuestRequest` and `GuestResponse`) + `version.rs:23` (`PROTOCOL_VERSION = 1`).
- captured-by: m80/docs/behaviors/wire-protocol/envelope.md#version-field + m80/m80-proto/tests/envelope/version.rs::request_and_response_carry_protocol_version

### Leaf: Carry an opaque request payload (program/args/env/cwd/timeout/stdin)
- parent_var: $L2_05_1
- labels: $ACTIVE,wire-protocol,envelope
- status: open
- behavior: The request envelope carries an opaque payload describing the exec call (`program`, `args`, optional `cwd`, `env`, `stdin`, optional `workspace_dir`, `timeout_ms`) without any tool name, identity, or policy field.
- source: dossier `02-sandbox-api-and-guest-proto.md` § Resolution (M80Request schema, lines 196-216).
- captured-by: m80/docs/behaviors/wire-protocol/envelope.md#request-payload + m80/m80-proto/tests/envelope/request_payload.rs::serializes_program_args_env_cwd_timeout

### Leaf: Carry response payload status, exit_code, stdout, stderr, timing
- parent_var: $L2_05_1
- labels: $ACTIVE,wire-protocol,envelope
- status: open
- behavior: The response envelope carries an opaque payload of `status` (Completed | TimedOut | Cancelled | Failed), optional `exit_code`, inline `stdout`, inline `stderr`, and `timing`, with no agent-tier identifier echoed back.
- source: dossier `02-sandbox-api-and-guest-proto.md` § Resolution (M80Response schema, lines 208-216).
- captured-by: m80/docs/behaviors/wire-protocol/envelope.md#response-payload + m80/m80-proto/tests/envelope/response_payload.rs::serializes_status_exit_stdout_stderr_timing

## L2-05.2 Frame transport (parent_var: $L2_05_2)

### Leaf: Frame envelopes as one NDJSON record per line
- parent_var: $L2_05_2
- labels: $ACTIVE,wire-protocol,framing
- status: open
- behavior: The transport serializes each envelope as a single NDJSON record (compact JSON terminated by a single `\n`) so parsers can read frames using line-buffered I/O.
- source: dossier `02-sandbox-api-and-guest-proto.md` § Guest protocol ("NDJSON over vsock"); predecessor `crates/sandbox/agent-guest-proto/src/envelope.rs:292-296` (`ndjson_serialize` pushes `b'\n'`).
- captured-by: m80/docs/behaviors/wire-protocol/framing.md#ndjson + m80/m80-proto/tests/framing/ndjson.rs::serializes_one_record_per_line

### Leaf: Reject any frame whose post-trim length exceeds 4 MiB
- parent_var: $L2_05_2
- labels: $ACTIVE,wire-protocol,framing
- status: open
- behavior: The deserializer rejects any frame whose post-trim payload exceeds 4 MiB (4 * 1024 * 1024 bytes) with an `OversizedPayload` error using a strict `>` comparison so an exactly-sized frame still passes.
- source: dossier `02-sandbox-api-and-guest-proto.md` § Guest protocol ("Max 4 MiB per frame"); predecessor `crates/sandbox/agent-guest-proto/src/envelope.rs:22` (`MAX_NDJSON_PAYLOAD_BYTES = 4 * 1024 * 1024`) and `301-306` (`> MAX_NDJSON_PAYLOAD_BYTES` check).
- captured-by: m80/docs/behaviors/wire-protocol/framing.md#size-cap + m80/m80-proto/tests/framing/size_cap.rs::rejects_frame_above_4mib_with_oversized_error

### Leaf: Treat malformed JSON as MalformedPayload and drop the connection
- parent_var: $L2_05_2
- labels: $ACTIVE,wire-protocol,framing
- status: open
- behavior: When `serde_json` fails to parse a frame the deserializer returns a `MalformedPayload` error carrying the parser detail, and the connection handler drops the connection rather than attempting to recover mid-stream.
- source: dossier `03-guest-daemon.md` § NDJSON envelope handling; predecessor `crates/sandbox/agent-guest-proto/src/envelope.rs:307-310` (`serde_json::from_slice ... MalformedPayload`) + `services/guestd-rs/src/main.rs:322-323` (logs and drops connection on error).
- captured-by: m80/docs/behaviors/wire-protocol/framing.md#parse-failure + m80/m80-proto/tests/framing/parse_failure.rs::malformed_json_returns_error_and_drops_connection

## L2-05.3 Version handshake (parent_var: $L2_05_3)

### Leaf: Exchange protocol version on every connection before any payload
- parent_var: $L2_05_3
- labels: $ACTIVE,wire-protocol,handshake
- status: open
- behavior: On every fresh connection peers exchange the `version` field before processing any application payload so an incompatible peer is rejected before doing exec work.
- source: predecessor `services/guestd-rs/src/main.rs:428-439` (`perform_stdio_handshake` reads handshake line, calls `negotiate_handshake`, writes response, flush).
- captured-by: m80/docs/behaviors/wire-protocol/handshake.md#exchange + m80/m80-proto/tests/handshake/exchange.rs::handshake_runs_before_first_request

### Leaf: Fail closed with IncompatibleVersion on any version mismatch
- parent_var: $L2_05_3
- labels: $ACTIVE,wire-protocol,handshake
- status: open
- behavior: A version mismatch — older or newer than the single live `PROTOCOL_VERSION` — produces `ProtoError::IncompatibleVersion { got, expected }` and the peer refuses to dispatch; there is no range-based fallback or shim.
- source: predecessor `crates/sandbox/agent-guest-proto/src/version.rs:23-37` (single `PROTOCOL_VERSION`, exact-match `is_compatible`); `envelope.rs:229-234` (returns `IncompatibleVersion`).
- captured-by: m80/docs/behaviors/wire-protocol/handshake.md#mismatch + m80/m80-proto/tests/handshake/mismatch.rs::rejects_incompatible_version

---

# L1-06 Vsock Channel

## L2-06.1 CID and port allocation (parent_var: $L2_06_1)

### Leaf: Derive guest CID deterministically from vm_id via FNV-1a hash
- parent_var: $L2_06_1
- labels: $ACTIVE,vsock,cid
- status: open
- behavior: Each VM's vsock guest CID is derived deterministically from `vm_id` via FNV-1a (32-bit) hashing into the dynamic CID range starting at 10_000, so the same `vm_id` always produces the same CID across restarts.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:41-53` (`guest_cid_for_vm_id`) + `MIN_DYNAMIC_GUESTD_GUEST_CID = 10_000`.
- captured-by: m80/docs/behaviors/vsock/cid.md#derivation + m80/m80-firecracker/tests/vsock/cid.rs::vm_id_derives_stable_cid

### Leaf: Keep guest CID below reserved range to avoid VMADDR_CID_ANY collision
- parent_var: $L2_06_1
- labels: $ACTIVE,vsock,cid
- status: open
- behavior: The CID derivation modulus is `u32::MAX - 1 - MIN` so the largest value produced is `u32::MAX - 2`, which keeps the guest CID strictly below the reserved vsock CIDs (`VMADDR_CID_ANY = u32::MAX`, `VMADDR_CID_HOST/HYPERVISOR` below MIN).
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:47-52` (FC-CORR-5 comment) + `tests` block `guestd_for_vm_uses_stable_non_reserved_cid` (lines 396-409).
- captured-by: m80/docs/behaviors/vsock/cid.md#reserved-range + m80/m80-firecracker/tests/vsock/cid.rs::cid_below_reserved_range

### Leaf: Pin guest port 9001 and host UDS at <run_dir>/vsock.sock
- parent_var: $L2_06_1
- labels: $ACTIVE,vsock,cid
- status: open
- behavior: The guest listens on the fixed vsock port 9001 (`DEFAULT_GUESTD_VSOCK_PORT`) and the host bridge socket lives at `<run_dir>/vsock.sock`, so the host always knows where to dial without per-VM port discovery.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:15` (`DEFAULT_GUESTD_VSOCK_PORT = 9001`) + `lib.rs:113-116` re-export; `services/guestd-rs/src/main.rs:280` (`bind_vsock_listener(config.vsock_port)`).
- captured-by: m80/docs/behaviors/vsock/cid.md#fixed-port + m80/m80-firecracker/tests/vsock/cid.rs::guest_port_is_fixed_at_9001

## L2-06.2 Ready-marker probe (parent_var: $L2_06_2)

### Leaf: Watch the serial console for the configured ready-marker token
- parent_var: $L2_06_2
- labels: $ACTIVE,vsock,ready
- status: open
- behavior: After `InstanceStart`, the host scans the per-VM serial console log for the ready-marker token and treats its first appearance as the signal that the guest daemon is bound and accepting vsock connections.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:697` (`wait_for_console_marker(config.ready_marker, config.ready_timeout)`) + `vsock.rs:14` (`DEFAULT_GUESTD_READY_MARKER = "GUESTD_READY"`).
- captured-by: m80/docs/behaviors/vsock/ready-marker.md#console-watch + m80/m80-firecracker/tests/vsock/ready_marker.rs::detects_marker_on_console

### Leaf: Read ready-marker token from the rootfs manifest
- parent_var: $L2_06_2
- labels: $ACTIVE,vsock,ready
- status: open
- behavior: The ready-marker token is configurable per image via the rootfs manifest's `ready_marker` field; the host uses the manifest value rather than hard-coding the default at probe time.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/backend.rs:122` (`ready_marker: DEFAULT_GUESTD_READY_MARKER.to_owned()`) overridden via `FirecrackerBackendConfig` + dossier `04-infra-and-artifacts.md` § Provenance manifest (`ready_marker` in manifest schema).
- captured-by: m80/docs/behaviors/vsock/ready-marker.md#manifest-token + m80/m80-firecracker/tests/vsock/ready_marker.rs::reads_marker_from_manifest

### Leaf: Bound ready-marker wait by ready_timeout (default 45s)
- parent_var: $L2_06_2
- labels: $ACTIVE,vsock,ready
- status: open
- behavior: The host waits for the ready marker for at most `ready_timeout` (default 45 seconds) before giving up and returning `VsockNotReady`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/backend.rs:27` (`DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(45)`) + `lifecycle.rs:697` (`wait_for_console_marker(..., config.ready_timeout)`).
- captured-by: m80/docs/behaviors/vsock/ready-marker.md#timeout + m80/m80-firecracker/tests/vsock/ready_marker.rs::times_out_after_ready_timeout

### Leaf: Run probe after InstanceStart and before any caller exec
- parent_var: $L2_06_2
- labels: $ACTIVE,vsock,ready
- status: open
- behavior: The ready probe runs after the `InstanceStart` API call returns and before any caller-visible exec is attempted, so callers never see a "not yet listening" connection failure on the first request.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:531-722` (preboot pipeline, ready wait between start and first vsock dial) per dossier `08-extraction-plan.md`.
- captured-by: m80/docs/behaviors/vsock/ready-marker.md#ordering + m80/m80-firecracker/tests/vsock/ready_marker.rs::probe_runs_after_start_before_exec

## L2-06.3 Connection lifecycle (parent_var: $L2_06_3)

### Leaf: Open one host-to-guest vsock connection per exec request
- parent_var: $L2_06_3
- labels: $ACTIVE,vsock,lifecycle
- status: open
- behavior: Each exec request opens a fresh host-side `UnixStream` to the per-VM bridge socket, sends `CONNECT <port>\n`, completes the request/response, then closes — connections are not pooled or reused.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:80-95,119-121` (`request_response` calls `connect_reader` + `send_connect` per call) + dossier `02-sandbox-api-and-guest-proto.md` line 188 ("NDJSON over vsock").
- captured-by: m80/docs/behaviors/vsock/connection.md#per-request + m80/m80-firecracker/tests/vsock/connection.rs::opens_fresh_connection_per_request

### Leaf: Validate the CONNECT acknowledgement starts with "OK <port>"
- parent_var: $L2_06_3
- labels: $ACTIVE,vsock,lifecycle
- status: open
- behavior: After sending `CONNECT <port>\n` the host reads a single ack line and rejects any line that does not start with `"OK "` followed by a parseable host port, surfacing `VsockBridgeProtocol` on malformed acknowledgements.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:130-152` (`send_connect` ack parsing) + tests `rejects_unexpected_connect_acknowledgement` lines 316-334.
- captured-by: m80/docs/behaviors/vsock/connection.md#connect-ack + m80/m80-firecracker/tests/vsock/connection.rs::rejects_malformed_connect_ack

### Leaf: Apply 5s read/write timeouts on each vsock bridge stream
- parent_var: $L2_06_3
- labels: $ACTIVE,vsock,lifecycle
- status: open
- behavior: Every bridge stream is set up with a 5-second default read and write timeout (`DEFAULT_VSOCK_BRIDGE_TIMEOUT`) so a wedged guest cannot stall the host indefinitely on any single read or write call.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/vsock.rs:13` (`DEFAULT_VSOCK_BRIDGE_TIMEOUT: Duration = Duration::from_secs(5)`) + `97-117` (`set_read_timeout` / `set_write_timeout`).
- captured-by: m80/docs/behaviors/vsock/connection.md#stream-timeouts + m80/m80-firecracker/tests/vsock/connection.rs::stream_has_default_timeouts

---

# L1-12 Configuration & Discovery

## L2-12.1 Env var schema (parent_var: $L2_12_1)

### Leaf: Discover firecracker and jailer binaries via M80_FIRECRACKER_BIN/_JAILER_BIN
- parent_var: $L2_12_1
- labels: $ACTIVE,configuration,env
- status: open
- behavior: m80 reads `M80_FIRECRACKER_BIN` and `M80_FIRECRACKER_JAILER_BIN` from the environment to locate the firecracker and jailer binaries, falling back to defaults under `/opt/m80/bin/` when unset.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:29-30` (`FIRECRACKER_BIN_ENV`, `JAILER_BIN_ENV`) and `services/sandbox-executor-rs/src/lib.rs` (`SANDBOX_EXECUTOR_FIRECRACKER_BIN/_JAILER_BIN` per dossier `05-consumers-and-integration-seams.md` § Config from env).
- captured-by: m80/docs/behaviors/configuration/env-schema.md#binaries + m80/m80-config/tests/env/binaries.rs::reads_firecracker_and_jailer_paths

### Leaf: Discover kernel and rootfs images via M80_FIRECRACKER_KERNEL_IMAGE/_ROOTFS_IMAGE
- parent_var: $L2_12_1
- labels: $ACTIVE,configuration,env
- status: open
- behavior: m80 reads `M80_FIRECRACKER_KERNEL_IMAGE` and `M80_FIRECRACKER_ROOTFS_IMAGE` to locate the boot artifacts, expecting absolute paths to a vmlinux file and an ext4 rootfs image respectively.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:31-32` (`FIRECRACKER_KERNEL_ENV`, `FIRECRACKER_ROOTFS_ENV`); dossier `05-consumers-and-integration-seams.md` § Config from env (`SANDBOX_EXECUTOR_FIRECRACKER_KERNEL_IMAGE/_ROOTFS_IMAGE`).
- captured-by: m80/docs/behaviors/configuration/env-schema.md#artifacts + m80/m80-config/tests/env/artifacts.rs::reads_kernel_and_rootfs_paths

### Leaf: Discover run-root via M80_FIRECRACKER_RUN_ROOT with documented default
- parent_var: $L2_12_1
- labels: $ACTIVE,configuration,env
- status: open
- behavior: m80 reads `M80_FIRECRACKER_RUN_ROOT` to locate the run-root directory; if unset, it falls back to the documented default (`DEFAULT_FIRECRACKER_RUN_ROOT`, currently `/tmp/predecessor-firecracker-run` upstream, renamed in m80 to a `/var/lib/m80-run`-style default).
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:36,38,298-300` (`FIRECRACKER_RUN_ROOT_ENV` + `DEFAULT_FIRECRACKER_RUN_ROOT` + `DiscoveryConfig::from_env` fallback); dossier `05-consumers-and-integration-seams.md` § Config from env.
- captured-by: m80/docs/behaviors/configuration/env-schema.md#run-root + m80/m80-config/tests/env/run_root.rs::reads_run_root_with_default

### Leaf: Read concurrency, jailer, and cgroup mode env keys
- parent_var: $L2_12_1
- labels: $ACTIVE,configuration,env
- status: open
- behavior: m80 reads `M80_FIRECRACKER_MAX_CONCURRENT_VMS`, `M80_FIRECRACKER_JAILER_MODE`, and `M80_FIRECRACKER_CGROUP_MODE` to govern admission, jailer enablement, and cgroup-v2 enforcement respectively.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/backend.rs:130-133` (`from_env` invokes `FirecrackerJailerMode::from_env` + `FirecrackerCgroupMode::from_env`); dossier `05-consumers-and-integration-seams.md` § Config from env (`SANDBOX_EXECUTOR_FIRECRACKER_MAX_CONCURRENT_VMS`).
- captured-by: m80/docs/behaviors/configuration/env-schema.md#modes + m80/m80-config/tests/env/modes.rs::reads_concurrency_jailer_and_cgroup_modes

## L2-12.2 Config loading order (parent_var: $L2_12_2)

### Leaf: Apply config sources in defaults → file → env → flags precedence
- parent_var: $L2_12_2
- labels: $ACTIVE,configuration,loading
- status: open
- behavior: m80 loads configuration in a deterministic precedence chain — built-in defaults, then `/etc/m80/config.toml`, then `~/.config/m80/config.toml`, then `M80_*` env vars, then command-line flags — with each later source overriding earlier values.
- source: dossier `09-cli-shape.md` § Configuration loading order (lines 192-200).
- captured-by: m80/docs/behaviors/configuration/loading-order.md#precedence + m80/m80-config/tests/loading/precedence.rs::flags_override_env_override_file_override_defaults

### Leaf: Layer system config under user config under env vars
- parent_var: $L2_12_2
- labels: $ACTIVE,configuration,loading
- status: open
- behavior: When both `/etc/m80/config.toml` and `~/.config/m80/config.toml` are present m80 layers them so the user-level file overrides the system-level file on a per-key basis, and env vars then override either.
- source: dossier `09-cli-shape.md` § Configuration loading order (lines 193-196).
- captured-by: m80/docs/behaviors/configuration/loading-order.md#layered-files + m80/m80-config/tests/loading/layered_files.rs::user_overrides_system_per_key

### Leaf: Reveal effective config via "m80 config show"
- parent_var: $L2_12_2
- labels: $ACTIVE,configuration,loading
- status: open
- behavior: The `m80 config show` subcommand prints the fully resolved configuration after all precedence rules are applied, including the source attribution for each key, so users can debug surprises.
- source: dossier `09-cli-shape.md` § Configuration loading order (line 199-200).
- captured-by: m80/docs/behaviors/configuration/loading-order.md#config-show + m80/m80-cli/tests/config/show.rs::reports_effective_config_with_sources

## L2-12.3 Backend construction (parent_var: $L2_12_3)

### Leaf: Build backend once via from_discovery_with_modes(discovery, jailer, cgroup)
- parent_var: $L2_12_3
- labels: $ACTIVE,configuration,construction
- status: open
- behavior: The Firecracker backend is constructed exactly once per process via `from_discovery_with_modes(discovery, jailer_mode, cgroup_mode)`, which materializes resolved `FirecrackerPaths` plus the run-root and timeout defaults from a single `DiscoveryConfig`.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/backend.rs:108-128` (`from_discovery_with_modes`) + dossier `05-consumers-and-integration-seams.md` § sandbox-executor-rs (singleton built once at service startup).
- captured-by: m80/docs/behaviors/configuration/backend-construction.md#single-build + m80/m80-firecracker/tests/backend/construction.rs::backend_built_once_per_process

### Leaf: Reuse the constructed backend across all subsequent VM lifecycles
- parent_var: $L2_12_3
- labels: $ACTIVE,configuration,construction
- status: open
- behavior: After construction the backend instance is reused for all subsequent VM-lifecycle calls within the process, sharing its admission semaphore and resolved binary paths so no per-request re-discovery is performed.
- source: dossier `05-consumers-and-integration-seams.md` § sandbox-executor-rs ("Singleton pattern: built once at service startup, reused for all requests").
- captured-by: m80/docs/behaviors/configuration/backend-construction.md#reuse + m80/m80-firecracker/tests/backend/construction.rs::backend_reused_across_requests

---

# L1-14 Concurrency / Admission / Run-Root Hygiene

## L2-14.1 Admission limiting (parent_var: $L2_14_1)

### Leaf: Gate VM creation by a tokio Semaphore wrapping the inner backend
- parent_var: $L2_14_1
- labels: $ACTIVE,concurrency,admission
- status: open
- behavior: The Firecracker backend is wrapped in an admission-limiting layer that holds an `Arc<Semaphore>` permit pool and grabs one owned permit per VM lifecycle, releasing it when the VM is torn down.
- source: predecessor `services/sandbox-executor-rs/src/lib.rs:1330-1354` (`AdmissionLimitedSandboxBackend::new` constructs `Semaphore::new(max_concurrent_vms.get())`).
- captured-by: m80/docs/behaviors/concurrency/admission.md#semaphore + m80/m80-firecracker/tests/admission/semaphore.rs::permit_acquired_per_vm

### Leaf: Read max-concurrent-VMs from M80_FIRECRACKER_MAX_CONCURRENT_VMS
- parent_var: $L2_14_1
- labels: $ACTIVE,concurrency,admission
- status: open
- behavior: The admission-limiting wrapper sizes its permit pool from `M80_FIRECRACKER_MAX_CONCURRENT_VMS` (positive non-zero integer), defaulting to a small documented value when the variable is unset.
- source: dossier `05-consumers-and-integration-seams.md` § Config from env (`SANDBOX_EXECUTOR_FIRECRACKER_MAX_CONCURRENT_VMS`); predecessor `services/sandbox-executor-rs/src/lib.rs:1330-1336` (`max_concurrent_vms: NonZeroUsize`).
- captured-by: m80/docs/behaviors/concurrency/admission.md#max-vms-env + m80/m80-firecracker/tests/admission/semaphore.rs::reads_max_from_env

### Leaf: Surface SandboxError::Unavailable when permit pool is exhausted
- parent_var: $L2_14_1
- labels: $ACTIVE,concurrency,admission
- status: open
- behavior: When the permit pool is exhausted `try_acquire_owned` fails immediately and the wrapper returns `SandboxError::Unavailable` carrying both the configured cap and the active-VM count rather than blocking the caller.
- source: predecessor `services/sandbox-executor-rs/src/lib.rs:1340-1353` (`try_admit` → `SandboxError::Unavailable { reason }`).
- captured-by: m80/docs/behaviors/concurrency/admission.md#exhausted + m80/m80-firecracker/tests/admission/semaphore.rs::reports_unavailable_when_pool_full

## L2-14.2 Run-root recovery loop (parent_var: $L2_14_2)

### Leaf: Spawn a background recovery task once at service startup
- parent_var: $L2_14_2
- labels: $ACTIVE,concurrency,recovery-loop
- status: open
- behavior: At service startup m80 spawns exactly one background task that drives `recover_stale_run_root` against the configured run-root, gated to only fire when the configured backend is the Firecracker backend.
- source: predecessor `services/sandbox-executor-rs/src/lib.rs:498` (`start_firecracker_run_root_recovery_loop` invocation) + `558-565` (returns `Some(tokio::spawn(...))` only for the Firecracker backend variant).
- captured-by: m80/docs/behaviors/concurrency/recovery-loop.md#spawn + m80/m80-firecracker/tests/recovery/loop_spawn.rs::spawns_recovery_task_for_firecracker_backend

### Leaf: Sleep 5 seconds between recovery passes
- parent_var: $L2_14_2
- labels: $ACTIVE,concurrency,recovery-loop
- status: open
- behavior: The recovery loop body sleeps for the constant `FIRECRACKER_RUN_ROOT_RECOVERY_INTERVAL` (5 seconds) between each recovery pass so the operation cost is bounded and predictable.
- source: predecessor `services/sandbox-executor-rs/src/lib.rs:69` (`FIRECRACKER_RUN_ROOT_RECOVERY_INTERVAL: Duration = Duration::from_secs(5)`) + `563-565` loop body.
- captured-by: m80/docs/behaviors/concurrency/recovery-loop.md#interval + m80/m80-firecracker/tests/recovery/loop_spawn.rs::recovery_interval_is_5s

### Leaf: Run recovery on a blocking task to avoid stalling the runtime
- parent_var: $L2_14_2
- labels: $ACTIVE,concurrency,recovery-loop
- status: open
- behavior: Each recovery pass runs `recover_stale_run_root` inside `tokio::task::spawn_blocking` so the synchronous filesystem walk does not occupy a tokio worker, and a panic in recovery is logged and the loop continues on the next interval.
- source: predecessor `services/sandbox-executor-rs/src/lib.rs:566-567,587-594` (`spawn_blocking` + warn-on-panic arm) + `lifecycle.rs:1477` (`pub fn recover_stale_run_root`).
- captured-by: m80/docs/behaviors/concurrency/recovery-loop.md#blocking-task + m80/m80-firecracker/tests/recovery/loop_spawn.rs::recovery_runs_on_blocking_task_and_survives_panics

### Leaf: Run a one-shot startup recovery before the periodic loop begins
- parent_var: $L2_14_2
- labels: $ACTIVE,concurrency,recovery-loop
- status: open
- behavior: Before the periodic loop ticks for the first time, m80 runs a one-shot synchronous recovery pass against the run-root so any orphans from a previous crash are reaped before the first new VM is admitted.
- source: predecessor `services/sandbox-executor-rs/src/lib.rs:1452-1462` (startup recovery `recover_stale_run_root(&backend.run_root)` with info-log on success) + `crates/sandbox/agent-sandbox-firecracker/src/lifecycle.rs:540` (lifecycle-time `recover_stale_run_root` call).
- captured-by: m80/docs/behaviors/concurrency/recovery-loop.md#startup-pass + m80/m80-firecracker/tests/recovery/startup.rs::startup_recovery_runs_before_first_admission

## L2-14.3 Stale-VM detection (parent_var: $L2_14_3)

### Leaf: Identify ownership via run-dir ownership.json marker file
- parent_var: $L2_14_3
- labels: $ACTIVE,concurrency,detection
- status: open
- behavior: A run directory is considered owned by m80 only when it contains a parseable `ownership.json` marker file at the run-dir root; directories without the marker are skipped during recovery and probing.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:39` (`RUN_OWNERSHIP_MARKER_FILE = "ownership.json"`) + `probe.rs:161-163` (`should_probe_run_dir` requires the marker).
- captured-by: m80/docs/behaviors/concurrency/stale-detection.md#ownership-marker + m80/m80-firecracker/tests/detection/ownership.rs::skips_run_dirs_without_marker

### Leaf: Combine lease liveness, API socket, and vsock probe to classify health
- parent_var: $L2_14_3
- labels: $ACTIVE,concurrency,detection
- status: open
- behavior: A VM is classified Healthy when its lease is live AND its API socket is reachable AND a vsock probe round-trip succeeds; missing any of the three drops it to Stuck or Degraded based on the failure-triage bundle.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/probe.rs:165-185` (`observe_owned_run_dir`) + `270-287` (`derive_probe_health` with three signals).
- captured-by: m80/docs/behaviors/concurrency/stale-detection.md#health-classification + m80/m80-firecracker/tests/detection/health.rs::classifies_health_from_three_signals

### Leaf: Preserve residue on ambiguity rather than deleting on uncertainty
- parent_var: $L2_14_3
- labels: $ACTIVE,concurrency,detection
- status: open
- behavior: When recovery cannot definitively confirm a run directory is dead — invalid lease, lease-unavailable error, or any I/O ambiguity — the run dir is preserved rather than deleted so live VMs are never destroyed by a recovery race.
- source: predecessor `crates/sandbox/agent-sandbox-firecracker/src/probe.rs:201-215` (`probe_lease_liveness` records `lease-unavailable` and returns false rather than erroring) + dossier `06-network-internals.md` lines 248-249 ("preserves ambiguous or live residue (don't delete on uncertainty)").
- captured-by: m80/docs/behaviors/concurrency/stale-detection.md#preserve-on-ambiguity + m80/m80-firecracker/tests/detection/ambiguity.rs::preserves_run_dir_when_lease_unavailable

## L2-14.4 Run-root layout invariants (parent_var: $L2_14_4)

### Leaf: Per-VM state lives under <run_root>/<vm_id>/
- parent_var: $L2_14_4
- labels: $ACTIVE,concurrency,layout
- status: open
- behavior: Every VM's per-instance state — sockets, ownership marker, lease, console log, network state, ownership.json — lives under `<run_root>/<vm_id>/`, with `vm_id` as the only directory name and no nested grouping.
- source: dossier `02-sandbox-api-and-guest-proto.md` and `08-extraction-plan.md` (run-dir layout); predecessor `crates/sandbox/agent-sandbox-firecracker/src/foundation.rs:39,407-439,660` (`VmPaths` derived from `<run_root>/<vm_id>` + `RUN_OWNERSHIP_MARKER_FILE` path).
- captured-by: m80/docs/behaviors/concurrency/layout.md#per-vm-dir + m80/m80-firecracker/tests/layout/per_vm_dir.rs::state_lives_under_run_root_slash_vm_id

### Leaf: Avoid cross-process collisions via sha256-of-run-root naming
- parent_var: $L2_14_4
- labels: $ACTIVE,concurrency,layout
- status: open
- behavior: Cross-process collisions on shared host resources (bridges, taps, etc.) are avoided by deriving names from `sha256(run_root_path)` so two m80 processes with distinct run-roots cannot pick the same bridge/tap name even by chance.
- source: dossier `06-network-internals.md` lines 47-69 (sha256 derivation of bridge/tap/MAC/IP from run-root path); predecessor `crates/sandbox/agent-sandbox-firecracker/src/network.rs:243-244` (sha256-of-run-root drives bridge naming).
- captured-by: m80/docs/behaviors/concurrency/layout.md#sha256-naming + m80/m80-firecracker/tests/layout/sha256_naming.rs::derives_unique_names_per_run_root
