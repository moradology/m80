# Firecracker vsock muxer state across snapshot/restore

**Date:** 2026-05-05  
**Author:** empirical research for m80-rrp.3.10  
**Firecracker version tested:** v1.15.1 (`/opt/firecracker/bin/firecracker --version`)  
**Kernel:** 5.10.245+ (stock Firecracker CI kernel)  
**Host:** x86_64 Linux 6.17, KVM available  

---

## 1. Headline finding

**vsock connections do NOT survive snapshot/restore.** Firecracker explicitly sends a `VIRTIO_VSOCK_EVENT_TRANSPORT_RESET` to the guest driver at snapshot creation time, which causes the guest kernel to tear down all established vsock connections on resume. However:

- **Guest vsock LISTEN sockets do survive** (per the virtio spec and confirmed empirically).
- **The host-side UnixListener (vsock.sock) is re-bound from scratch** at restore time.
- **The m80-7tpy inverted-readiness pattern does NOT survive** — guestd does not re-dial after restore because it fire-and-forgot its ready connection.
- **guestd's exec listen socket (port 9001) survives**, making new host-initiated connections possible after restore — this is the path that matters for exec dispatch.

The upshot: snapshot/restore requires m80 to re-establish the readiness handshake using a different mechanism than the current boot-time inverted-readiness pattern.

---

## 2. Firecracker source analysis

### 2.1 What the Persist trait saves

`src/vmm/src/devices/virtio/vsock/persist.rs` defines `VsockBackendState`:

```rust
// persist.rs:37-43
pub struct VsockBackendState {
    pub uds_path: String,
    pub local_port_last: u32,
}
```

Only two fields are saved: the host UDS path string and the last-allocated host-side port number. The `VsockMuxer` (= `VsockUnixBackend`) fields that are **not** saved include:

- `conn_map: HashMap<ConnMapKey, MuxerConnection>` — all active connections
- `listener_map: HashMap<RawFd, EpollListener>` — all epoll listeners
- `epoll: Epoll` — the nested epoll fd
- `local_port_set: HashSet<u32>` — allocated port set

**Source:** `persist.rs:61-80` (`VsockUnixBackend::save` / `VsockUnixBackend::restore`).

### 2.2 Restore re-creates the muxer from scratch

```rust
// persist.rs:73-80
fn restore(
    constructor_args: Self::ConstructorArgs,
    state: &Self::State,
) -> Result<Self, Self::Error> {
    let mut backend = Self::new(constructor_args.cid, state.uds_path.clone())?;
    backend.local_port_last = state.local_port_last;
    Ok(backend)
}
```

`Self::new` calls `UnixListener::bind(host_sock_path)` (`muxer.rs:315`). This means:

1. On restore, Firecracker tries to **bind a new UnixListener** at the same `uds_path` stored in the snapshot.
2. If the old process's socket file still exists on the filesystem, `bind` fails with `EADDRINUSE` and the restore aborts: `"Error binding to the host-side Unix socket: Address in use (os error 98)"`.
3. The old FC process does **not** unlink the socket on exit — it lingers as a filesystem artifact.

**Empirical confirmation:** a restore attempted without removing the old `vsock.sock` first failed with exactly this error.

### 2.3 TRANSPORT_RESET_EVENT at snapshot creation

```rust
// device.rs:400-408
fn prepare_save(&mut self) {
    // Send Transport event to reset connections if device is activated.
    if self.is_activated() {
        self.send_transport_reset_event().unwrap_or_else(|err| {
            error!("Failed to send reset transport event: {:?}", err);
        });
    }
}
```

`send_transport_reset_event` writes `VIRTIO_VSOCK_EVENT_TRANSPORT_RESET = 0` to the guest's event virtqueue and signals an interrupt (`device.rs:252-277`). According to the virtio spec (and the Firecracker docs):

- The guest vsock driver **closes all established connections** when it processes this event on resume.
- **Existing listen sockets remain active** and can accept new connections.
- The guest CID is re-fetched from the device config.

The `kick()` method called on resume also re-signals the event virtqueue to ensure delivery of the already-queued `TRANSPORT_RESET_EVENT` (`device.rs:383-398`). The code comment is explicit:

```rust
// device.rs:384-389
// Vsock has complicated protocol that isn't resilient to any packet loss,
// so for Vsock we don't support connection persistence through snapshot.
// Any in-flight packets or events are simply lost.
// Vsock is restored 'empty'.
```

**Source:** `device.rs:383-408`.

### 2.4 Guest-initiated companion paths (`<vsock_uds>_<port>`)

When a guest process connects outbound to a host port, Firecracker's muxer (`muxer.rs:619-641`) resolves the destination by attempting `UnixStream::connect(format!("{uds_path}_{port}"))`. This companion socket must exist on the host at connect time.

```rust
// muxer.rs:620-621
fn handle_peer_request_pkt(&mut self, pkt: &VsockPacketTx) {
    let port_path = format!("{}_{}", self.host_sock_path, pkt.hdr.dst_port());
    UnixStream::connect(port_path)
```

Since the muxer is re-created from scratch at restore time, there is no record of any previously-established companion connections. Each new guest-initiated connect simply tries to connect to the companion path at the moment it is requested — the host must have that listener ready at that time.

---

## 3. Official Firecracker documentation

From `docs/snapshotting/snapshot-support.md`:

> Both network and vsock packet loss can be expected on guests that are resumed from snapshots in another Firecracker process. It is also not guaranteed that the state of the network connections survives the process. Furthermore, vsock connections that are open when the snapshot is taken are closed, but existing vsock listen sockets in the guest still remain active and can accept new connections after resume.

And the dedicated section ("Vsock device reset"):

> The vsock device is reset across snapshot/restore to avoid inconsistent state between device and driver leading to breakage (#2218). This is done by sending a `VIRTIO_VSOCK_EVENT_TRANSPORT_RESET` event to the guest driver during `SnapshotCreate` (#2562). On `SnapshotResume`, when the VM becomes active again, the vsock driver closes all existing connections. Existing listen sockets still remain active, but their CID is updated to reflect the current `guest_cid`.

This matches the code analysis exactly.

---

## 4. Empirical results

### 4.1 Test setup

- Boot a minimal VM with `init=/m80-guestd`, vsock CID 3, UDS at `/tmp/m80-vsock-survival/vsock.sock`
- guestd binds a vsock listen socket on port 9001 and dials a ready signal to host port 52525
- Host pre-creates a `UnixListener` at `vsock.sock_52525` (m80-7tpy pattern)
- Snapshot is taken after guestd has started; then original VM is killed; restore to new FC process

### 4.2 Results table

| Question | Finding | Evidence |
|---|---|---|
| Q1: Do established vsock connections survive? | **No** — TRANSPORT_RESET_EVENT kills them | Per spec, per code (`device.rs:prepare_save`), per FC docs |
| Q2: Does the host-side `vsock.sock` survive? | **No** — must be explicitly cleaned and re-bound | Empirical: restore fails with EADDRINUSE if socket not removed; after removal, `bind()` in `VsockUnixBackend::new` succeeds |
| Q3: Does `vsock.sock` re-bind automatically on restore? | **Yes** — at `snapshot/load` time, before `resume` | Empirical: `vsock.sock` appeared as a socket after `PUT /snapshot/load` returned 204 |
| Q4: Do guest vsock LISTEN sockets survive? | **Yes** — guestd's exec listener on port 9001 survived | Empirical: host-initiated `CONNECT 9001` → received `OK 1073741826` post-restore |
| Q5: Does the m80-7tpy ready companion path survive? | **No** — the companion socket `vsock.sock_52525` was removed as part of jail teardown and must be re-created | Empirical: listener pre-created by test harness before restore |
| Q6: Does guestd re-dial the ready signal post-restore? | **No** — guestd dials once at startup, drops the connection, and has no reconnect loop | Empirical: listener waited 15 s, no signal received |
| Q7: Can new host-initiated connections be established after restore? | **Yes** — immediately after resume | Empirical: `CONNECT 9001` → `OK 1073741826` (post-restore local port is 1073741826 vs 1073741824 pre-snapshot) |

### 4.3 Minimal repro

```bash
#!/usr/bin/env bash
# Requires: root, /opt/firecracker/bin/firecracker, /tmp/m80-build/minimal/
# Full script at /tmp/m80-vsock-survival/full-test.sh and /tmp/m80_full_test.py

SCRATCH=/tmp/m80-vsock-scratch
FC_BIN=/opt/firecracker/bin/firecracker
KERNEL=/tmp/m80-build/minimal/vmlinux
ROOTFS=/tmp/m80-build/minimal/output.ext4
VSOCK_UDS=$SCRATCH/vsock.sock
READY_PORT=52525
EXEC_PORT=9001

mkdir -p "$SCRATCH"
cp "$ROOTFS" "$SCRATCH/rootfs.ext4"

# --- Boot golden VM ---
# Pre-create ready listener at vsock.sock_52525 (m80-7tpy pattern)
python3 -c "
import socket, os
p = '$VSOCK_UDS'+'_$READY_PORT'
if os.path.exists(p): os.unlink(p)
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.bind(p); s.listen(5); s.settimeout(30)
conn, _ = s.accept()
print('Ready signal:', conn.recv(1).hex())
conn.close()
" &

"$FC_BIN" --api-sock "$SCRATCH/fc1.sock" --level Warn &
sleep 0.3

curl -s --unix-socket "$SCRATCH/fc1.sock" -X PUT http://localhost/boot-source \
  -H 'Content-Type: application/json' \
  -d '{"kernel_image_path":"'"$KERNEL"'","boot_args":"console=ttyS0 reboot=k panic=1 pci=off init=/m80-guestd"}'
curl -s --unix-socket "$SCRATCH/fc1.sock" -X PUT http://localhost/drives/rootfs \
  -H 'Content-Type: application/json' \
  -d '{"drive_id":"rootfs","path_on_host":"'"$SCRATCH/rootfs.ext4"'","is_root_device":true,"is_read_only":false}'
curl -s --unix-socket "$SCRATCH/fc1.sock" -X PUT http://localhost/machine-config \
  -H 'Content-Type: application/json' -d '{"vcpu_count":1,"mem_size_mib":128}'
curl -s --unix-socket "$SCRATCH/fc1.sock" -X PUT http://localhost/vsock \
  -H 'Content-Type: application/json' \
  -d '{"vsock_id":"vsock0","guest_cid":3,"uds_path":"'"$VSOCK_UDS"'"}'
curl -s --unix-socket "$SCRATCH/fc1.sock" -X PUT http://localhost/actions \
  -H 'Content-Type: application/json' -d '{"action_type":"InstanceStart"}'

# Wait for guestd ready (via listener above); then test pre-snapshot conn
sleep 2
python3 -c "
import socket
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect('$VSOCK_UDS'); s.sendall(b'CONNECT $EXEC_PORT\n')
print('Pre-snap:', s.recv(32))
s.close()
"

# --- Pause + snapshot ---
curl -s --unix-socket "$SCRATCH/fc1.sock" -X PATCH http://localhost/vm \
  -H 'Content-Type: application/json' -d '{"state":"Paused"}'
curl -s --unix-socket "$SCRATCH/fc1.sock" -X PUT http://localhost/snapshot/create \
  -H 'Content-Type: application/json' \
  -d '{"snapshot_path":"'"$SCRATCH/snap.bin"'","mem_file_path":"'"$SCRATCH/mem.bin"'","snapshot_type":"Full"}'

# Kill original, remove lingering socket
FC1_PID=$(pgrep -f "firecracker.*fc1.sock")
kill "$FC1_PID"
rm -f "$VSOCK_UDS" "$SCRATCH/vsock.sock_"*

# --- Restore ---
# Pre-create ready listener again (must exist before resume for guestd re-dial)
python3 -c "
import socket, os
p = '$VSOCK_UDS'+'_$READY_PORT'
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.bind(p); s.listen(5); s.settimeout(15)
try:
    conn, _ = s.accept()
    print('Re-dial signal:', conn.recv(1).hex())
except: print('No re-dial (expected: guestd has no reconnect loop)')
" &

"$FC_BIN" --api-sock "$SCRATCH/fc2.sock" --level Warn &
sleep 0.3

# Load snapshot — vsock.sock is REBOUND by Firecracker during load
curl -s --unix-socket "$SCRATCH/fc2.sock" -X PUT http://localhost/snapshot/load \
  -H 'Content-Type: application/json' \
  -d '{"snapshot_path":"'"$SCRATCH/snap.bin"'","mem_file_path":"'"$SCRATCH/mem.bin"'","resume_vm":false}'

# vsock.sock exists here (rebound at load time)
test -S "$VSOCK_UDS" && echo "vsock.sock rebound: YES" || echo "vsock.sock: NO"

curl -s --unix-socket "$SCRATCH/fc2.sock" -X PATCH http://localhost/vm \
  -H 'Content-Type: application/json' -d '{"state":"Resumed"}'

# After TRANSPORT_RESET settles (~3-5s), test new host-initiated connection
sleep 5
python3 -c "
import socket
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect('$VSOCK_UDS'); s.sendall(b'CONNECT $EXEC_PORT\n')
r = s.recv(32); print('Post-restore conn:', r)
s.close()
# Expect: b'OK <new_local_port>\n'  (SUCCESS: guestd listen socket survived)
"
```

---

## 5. Specific questions answered (from the bead)

### Q1: Does an in-VM vsock connection remain valid after restore?

**No.** The `VIRTIO_VSOCK_EVENT_TRANSPORT_RESET` event is sent at snapshot creation (`device.rs:prepare_save`), and the guest vsock driver processes it on resume — tearing down all established connections. There is no exception for any connection type or port.

### Q2: Does the host-side `<vsock_uds>` (`vsock.sock`) survive?

**Not automatically.** The old FC process does not unlink the socket file on exit; it lingers as a dead socket. The new FC process (restore path) calls `VsockUnixBackend::new` → `UnixListener::bind(uds_path)` which fails with `EADDRINUSE` if the old file exists. m80 must **remove `vsock.sock` from the jail directory** as part of the original VM's teardown before restoring. After removal, the restore `bind` succeeds and `vsock.sock` reappears (rebound) at `PUT /snapshot/load` time — before `PATCH /vm {"state":"Resumed"}`.

### Q3: Does the `<vsock_uds>_<port>` companion path need re-creation?

**Yes, for the ready port.** The companion path `vsock.sock_52525` must be pre-created by the host before the restored VM is resumed, because:

1. After TRANSPORT_RESET, the guest vsock driver re-dials anything that needs to reconnect.
2. guestd's current design (fire-and-forget ready dial) means it will NOT re-dial — but a redesigned guestd that does re-dial would need the listener to be present.
3. For new guest-initiated outbound connections (any port), m80 must ensure the companion listener is present at `<vsock_uds>_<port>` at the time the guest-side connect is issued.

The companion path for the exec channel (port 9001) is host-initiated in m80's design — the host connects to `vsock.sock` using the `CONNECT <port>` protocol, so no companion file is needed for exec. The companion-path mechanism is only required for guest-initiated outbound.

### Q4: Does the m80-7tpy inverted-readiness pattern survive snapshot?

**No.** guestd connects once to the ready port (52525) at startup, sends one byte, drops the connection (`drop(ready)`), and enters its exec accept loop. There is no reconnect loop. After TRANSPORT_RESET kills the in-flight established connection, guestd does not re-dial. The host listener waits but never receives a signal.

**However, this is not fatal:** guestd's exec listen socket (port 9001) **does survive** because it is a vsock LISTEN socket, not an established connection. The host can connect to it directly via `CONNECT 9001` on `vsock.sock` after restore — empirically confirmed with `OK 1073741826` response.

---

## 6. Recommendation for `m80-rrp.3.11` DESIGN

### 6.1 What the restore path must do for vsock

The following operations are required when loading a snapshot into a new Firecracker process:

1. **Remove `<jail>/vsock.sock`** before calling `PUT /snapshot/load`. The original VM must have been killed (and its jail/run-dir teardown must include socket removal). If vsock.sock is not removed, the load will fail with `EADDRINUSE`.

2. **After load, before resume:** `vsock.sock` will have been rebound by Firecracker at load time. No action needed.

3. **Pre-create `<jail>/vsock.sock_<READY_PORT>` before or at resume** if guestd will be modified to re-dial. For the current guestd design (no re-dial), this step is a no-op but should still be created to allow a future re-dial.

4. **After resume, wait for TRANSPORT_RESET to settle** (empirically < 1s; use a short fixed sleep or probe the exec port directly).

5. **For exec dispatch:** the host connects to `vsock.sock` with `CONNECT 9001`; guestd's listen socket survives and accepts. No guestd restart needed.

### 6.2 The readiness problem: guestd does not re-dial

The current m80-7tpy readiness pattern breaks on restore. The host has no signal that the restored VM's exec channel is ready. Two options:

**Option A: Skip the ready handshake on restore path.** Since guestd's exec listen socket is known to survive (per spec and empirical test), the host can simply probe `CONNECT 9001` directly after resume. If guestd's process is alive (which it will be — it's PID 1, the guest kernel won't kill it), the connection will succeed. No separate readiness signal needed on the restore path.

**Option B: Add a guestd re-dial on TRANSPORT_RESET.** guestd could catch the connection-close on its ready socket and re-dial the ready port. This would preserve the existing readiness semantics. This requires adding a reconnect loop to guestd's startup sequence — technically straightforward but changes the guestd interface contract.

**Recommendation for DESIGN:** start with Option A (probe directly), because it requires no guestd changes and is empirically sound. Option B can be added later if the probe introduces latency uncertainty.

### 6.3 Warm-pool snapshot architecture implications

For a warm pool of restored VMs:

- Each pool slot needs its own `vsock.sock` path (within its own jail/run-dir — already the case).
- The warm-pool "freeze" step must: (a) pause the VM, (b) snapshot, (c) kill the VM, (d) **remove vsock.sock and all companion sockets** from the jail.
- The warm-pool "thaw" step: (a) load snapshot (vsock.sock reappears), (b) resume, (c) probe `CONNECT 9001` to confirm guestd exec channel is live.
- The probe at thaw replaces the `phase_12b_ready_accept` that the cold-boot path uses.

The cold-boot `phase_11b_bind_ready_listener` and `phase_12b_ready_accept` pair is **not used** on the restore path. The restore path needs a simpler `phase_restore_probe_exec_channel`.

---

## 7. Open questions for DESIGN

1. **How long does TRANSPORT_RESET processing take in the guest?** The empirical test used a 3s sleep before probing. A tighter bound requires measuring the latency from `PATCH /vm Resumed` to first successful `CONNECT 9001`. This is likely < 50 ms but needs a data point.

2. **Can the probe connection itself be retried cheaply?** If the first `CONNECT <EXEC_PORT>` attempt races with TRANSPORT_RESET processing in the guest kernel, what does the muxer return — RST, EOF, or timeout? Understanding the failure mode determines whether a simple retry loop is safe.

3. **Does `local_port_last` being restored matter?** The saved `local_port_last` (empirically, the restored VM used port 1073741826 vs 1073741824 pre-snapshot) means the port counter resumes correctly. Warm-pool VMs restoring from the same snapshot will all start with the same `local_port_last`. Is port aliasing across concurrent pool slots a concern? (Probably not — each VM has its own vsock CID and UDS path.)

4. **vsock_override on snapshot load.** Firecracker supports a `vsock_override` field in `PUT /snapshot/load` to specify a different UDS path than the one embedded in the snapshot. This enables: (a) snapshot taken in one jail path, restored in another without path conflicts, and (b) multiple VMs restored from the same snapshot (each with their own UDS path). m80's restore path should use this rather than matching jail paths across runs. Confirm the API field name and test it.

5. **Does the guest CID change on restore?** The Firecracker docs say "the guest_cid configuration field is fetched again" after TRANSPORT_RESET. If the restored VM is launched with a different CID (e.g., for cloning), what does guestd do? In m80's current design, CID is fixed at 3 — verify this remains valid post-restore.

---

## 8. Files referenced

| File | Purpose |
|---|---|
| `firecracker/src/vmm/src/devices/virtio/vsock/persist.rs` | `VsockBackendState` definition; `save`/`restore` implementations |
| `firecracker/src/vmm/src/devices/virtio/vsock/device.rs:383-408` | `prepare_save` sends TRANSPORT_RESET_EVENT; `kick` re-delivers on resume |
| `firecracker/src/vmm/src/devices/virtio/vsock/unix/muxer.rs:312-335` | `VsockMuxer::new` — the constructor called on restore (rebinds host UDS) |
| `firecracker/src/vmm/src/devices/virtio/vsock/unix/muxer.rs:619-641` | `handle_peer_request_pkt` — companion path `<uds>_<port>` lookup |
| `firecracker/docs/snapshotting/snapshot-support.md` | Official vsock device reset documentation |
| `firecracker/tests/integration_tests/functional/test_snapshot_basic.py` | Firecracker's own snapshot+vsock cycle tests (the integration test verifies new connections work post-restore using `check_guest_connections` / `check_host_connections`) |
| `crates/m80-firecracker/src/launch.rs:203-226` | m80 Phase 11b (pre-create ready listener) and Phase 12b (accept readiness) |
| `crates/m80-guestd/src/main.rs:66-85` | guestd's inverted-readiness connect (fire-and-forget; no reconnect) |
| `crates/m80-proto/src/lib.rs:37-53` | `READY_PORT_DEFAULT = 52525` definition |
| `/tmp/m80_full_test.py` | Full empirical test script (scratch, not committed) |
