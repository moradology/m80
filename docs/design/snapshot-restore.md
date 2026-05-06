# Snapshot-restore design

**Bead:** m80-rrp.3.11  
**Status:** design locked (Wave 1)  
**Empirical basis:** `docs/exploration/firecracker-vsock-snapshot.md` (m80-rrp.3.10)  
**Implementation target:** m80-rrp.3.4 / m80-rrp.3.13 (Wave 3)

---

## 1. What this document covers

This document locks the host-side state machine for snapshot capture and
restore, the vsock semantics that constrain both paths, the snapshot file
layout, and the failure modes the orchestrator must handle.

It does NOT contain implementation code. The `Sandbox::capture` and
`launch_from_snapshot` methods live in `m80-firecracker` (m80-rrp.3.4 /
m80-rrp.3.13). The REST methods they call (`put_snapshot_create`,
`put_snapshot_load`, `patch_vm_state`) are in `m80-firecracker-client`
(m80-rrp.3.12, delivered in Wave 1).

---

## 2. Snapshot file layout

A Firecracker snapshot is a two-file pair plus the caller-managed disk images:

| File | Contents | Who writes it |
|---|---|---|
| `vm.snap` | microVM state: vCPU registers, device state, vsock muxer state | Firecracker |
| `mem.snap` | Guest physical memory | Firecracker |
| `rootfs.ext4` | Root disk (copy-on-write guest writes DO NOT appear here) | m80 (pre-existing) |
| `workspace.ext4` | Workspace overlay disk (if attached) | m80 (pre-existing) |

Firecracker saves the CID, drive paths, network interface TAP device name, and
vsock UDS path inside `vm.snap`. On restore, Firecracker recreates the vsock
muxer by calling `UnixListener::bind(saved_uds_path)` — the UDS path is
re-provisioned, not inherited. Drive and network device state is also restored
from the snapshot; the host does not need to re-issue PUT /drives or PUT
/network-interfaces on the restore path.

The snapshot pair is written to a caller-chosen directory. m80 uses:

```
{snapshot_dir}/vm.snap
{snapshot_dir}/mem.snap
```

The snapshot directory must be accessible (and writable at capture time) by
the Firecracker process inside the jail. `m80-firecracker` implements this by
bind-mounting the caller's host snapshot directory into the jail at
`/snapshot` and translating the API payload to `/snapshot/vm.snap` and
`/snapshot/mem.snap`.

---

## 3. Capture path

The host-side capture sequence is:

```
1. Assert VM is running (Firecracker state: Running)
2. Reject or drain in-flight exec requests (see §3.1)
3. PATCH /vm {"state": "Paused"}          — pause vCPU execution
4. PUT /snapshot/create {snapshot_path, mem_file_path, snapshot_type: "Full"}
5. (optional) PATCH /vm {"state": "Resumed"}  — resume if the VM continues
   OR kill the Firecracker process           — if the VM is being frozen
6. Remove vsock.sock from the jail directory (mandatory before any restore)
```

The wire shape for step 4:

```json
PUT /snapshot/create HTTP/1.1
Content-Type: application/json

{
  "snapshot_path": "/run/m80/vms/<vm-id>/snapshots/vm.snap",
  "mem_file_path": "/run/m80/vms/<vm-id>/snapshots/mem.snap",
  "snapshot_type": "Full"
}
```

### 3.1 In-flight exec policy

At the moment `Sandbox::capture` is called, there may be an exec request in
flight inside the guest. The default policy is **reject**: `capture` fails if
any exec is in progress. A caller-supplied `force: bool` flag bypasses the
check; the exec's output will be lost. The policy is enforced by
`m80-firecracker` before the pause; it does not require guestd cooperation.

### 3.2 At snapshot creation time, vsock connections are reset

Firecracker sends `VIRTIO_VSOCK_EVENT_TRANSPORT_RESET` to the guest vsock
driver at the moment `PUT /snapshot/create` is processed (in `prepare_save`).
When the VM is resumed from this snapshot, the guest kernel tears down all
established connections. Guest vsock LISTEN sockets survive.

This is Firecracker's explicit design choice (see empirical doc §2.3). It
cannot be avoided; the snapshot always contains the reset event queued.

---

## 4. Restore path

The host-side restore sequence is:

```
1. Assert vsock.sock is absent from the jail directory (remove if present)
2. Start a new Firecracker process against the same jail
3. PUT /snapshot/load {snapshot_path, mem_backend: {File, mem.snap}, resume_vm: false}
   — vsock.sock is rebound by Firecracker during load (before 204 returns)
4. (optional) Set up TAP device / outbound NAT if net is required (see §4.1)
5. PATCH /vm {"state": "Resumed"}
6. phase_restore_probe_exec_channel: CONNECT 9001 on vsock.sock
   — retry loop with short backoff; success confirms guestd exec loop is live
```

The wire shape for step 3:

```json
PUT /snapshot/load HTTP/1.1
Content-Type: application/json

{
  "snapshot_path": "/run/m80/vms/<vm-id>/snapshots/vm.snap",
  "mem_backend": {
    "backend_type": "File",
    "backend_path": "/run/m80/vms/<vm-id>/snapshots/mem.snap"
  },
  "resume_vm": false
}
```

Using `resume_vm: false` keeps the sequence explicit: load, provision host
resources, then resume. This avoids a race where the guest vCPUs start before
the host TAP or NAT rules are ready.

### 4.1 Host resource re-provisioning on restore

Firecracker restores the drive and network device state from the snapshot.
However, host-side resources are not persistent across Firecracker processes:

| Resource | Restored by FC? | Host action required |
|---|---|---|
| Drive file content | Implicit (file on disk) | None — file persists |
| TAP device | No | Re-create TAP, re-apply NAT rules |
| vsock UDS | Yes (rebind) | Remove stale vsock.sock first |
| vsock companion sockets | No | Pre-create if guestd re-dials |

### 4.2 vsock UDS pre-removal (mandatory)

The original Firecracker process does not unlink `vsock.sock` on exit. The
socket file lingers as a dead socket. When `PUT /snapshot/load` runs,
Firecracker calls `UnixListener::bind(saved_uds_path)`, which fails with
`EADDRINUSE` if the file exists:

```
Error binding to the host-side Unix socket: Address in use (os error 98)
```

The orchestrator must call `unlink(vsock.sock)` as part of the original VM's
teardown, before initiating any restore. This is the `m80-firecracker`
responsibility; it must not be silently omitted.

`vsock_override` in `PUT /snapshot/load` lets the caller redirect to a
different UDS path — useful when restoring from a snapshot taken in a
different jail, or when bringing up multiple VMs from the same snapshot in
different slots. Each slot already has its own jail directory in m80's design,
so the override is optional when the jail path is consistent.

### 4.3 Readiness handshake on the restore path

The cold-boot ready handshake (`phase_11b_bind_ready_listener` +
`phase_12b_ready_accept`) is NOT used on the restore path.

The cold-boot path works as follows: guestd, on first boot, dials the host's
ready listener (port 52525), sends one byte, and drops the connection. The
host waits for this signal before declaring the VM ready.

On restore, after `VIRTIO_VSOCK_EVENT_TRANSPORT_RESET` processes in the guest
kernel, the established ready connection is torn down. guestd's current design
(fire-and-forget, no reconnect loop) means it does not re-dial. The host
listener would wait indefinitely.

The restore path uses a direct probe instead:

**`phase_restore_probe_exec_channel`**: the host connects to `vsock.sock` and
sends `CONNECT 9001\n`. If guestd's exec LISTEN socket is alive (empirically:
it always is, because listen sockets survive TRANSPORT_RESET), the muxer
forwards the connection and guestd responds `OK <port>\n`. The host closes the
probe connection and declares the VM ready.

The probe replaces the readiness signal entirely. No guestd modification is
required.

**Retry policy:** the probe may race with TRANSPORT_RESET processing in the
guest kernel (event is queued at snapshot time, delivered on resume). Use a
short retry loop: attempt `CONNECT 9001`, tolerate `RST`/`EOF` from the muxer,
sleep 50 ms, retry. Cap at a timeout consistent with the rest of the launch
timeout budget (empirically the settle time is < 1 s; a 5 s cap is safe).

---

## 5. CID determinism on restore

The vsock CID is embedded in the snapshot's device state. When
`PUT /snapshot/load` completes, the restored guest VM has the same CID it had
at snapshot time.

m80 derives CIDs deterministically via `cid_for_vm_id(vm_id)`. On restore, the
orchestrator must use the same `vm_id` as the original VM so that the CID
matches what is in the snapshot. A mismatch would cause the muxer to associate
traffic with the wrong guest CID, breaking outbound NAT.

For warm-pool VMs restored from a shared base snapshot, each slot must be
assigned a consistent `vm_id` before snapshot creation and re-used on every
restore of that slot. The alternative — using `vsock_override` to redirect the
UDS path — does not change the CID embedded in the snapshot, so all pool slots
restored from the same snapshot would share the same CID. That only works if
each slot has a separate physical jail (separate UDS path, separate TAP device)
AND the CID collision is acceptable at the vsock layer. For now, m80's design
uses one CID per vm_id, so slots must have distinct vm_ids.

---

## 6. Compat checks on restore

Before issuing `PUT /snapshot/load`, the orchestrator should verify that the
host environment is compatible with the snapshot. The following checks apply:

| Check | Required | Notes |
|---|---|---|
| Firecracker version | Advisory | Snapshots are versioned; FC warns on minor-version mismatch but loads anyway. Major-version mismatch may abort. |
| Kernel image SHA | Advisory | The snapshot saves CPU state; a different kernel image on restore is undefined behavior, but FC does not verify this. |
| `mem_size_mib` | Hard | FC verifies the mem file size matches the snapshot's recorded memory size; mismatch → load error. |
| `vcpu_count` | Hard | vCPU register state is per-vCPU; count mismatch → load error. |
| KVM version / host CPU | Hard | Snapshots are not portable across microarchitectures; FC returns an error if the host CPU cannot replay the saved state. |

Advisory checks are logged as warnings; hard checks abort the restore with an
appropriate error variant.

---

## 7. Failure modes

| Failure | Observable symptom | Handling |
|---|---|---|
| `vsock.sock` not removed before load | `PUT /snapshot/load` → 400 `"EADDRINUSE"` | Orchestrator must unlink before load; surface as `SnapshotLoadFailed` |
| Snapshot file missing or unreadable | `PUT /snapshot/load` → 400 | Surface as `SnapshotLoadFailed`; do not retry |
| FC version mismatch (minor) | `PUT /snapshot/load` → 204 with warning logged | Load succeeds; orchestrator continues |
| FC version mismatch (major) | `PUT /snapshot/load` → 400 | Surface as `SnapshotLoadFailed` |
| `kvm-no-immediate-exit` unsupported | Firecracker aborts at startup | FC docs warn: some kernel versions do not support the KVM extension required for efficient vCPU save/restore; the FC binary will refuse to start or emit an error at load time |
| Partial restore (mem file truncated) | `PUT /snapshot/load` → 400 or silent guest corruption | Surface as `SnapshotLoadFailed`; a truncated mem file must be rejected, not silently padded |
| Probe timeout after resume | `phase_restore_probe_exec_channel` exhausts retries | Surface as a typed orchestrator error (not a client error); the VM is likely dead (OOM or guestd crash) |
| Drive file missing at restore site | Guest kernel panic on first disk access | Pre-flight: verify drive file exists before `PUT /snapshot/load` |

---

## 8. Out of scope for this leaf

The following are explicitly deferred to Wave 3 (m80-rrp.3.4, m80-rrp.3.13):

- `Sandbox::capture` implementation in `m80-firecracker`
- `launch_from_snapshot` / `Sandbox::restore` implementation
- Warm-pool orchestration (pool manager, freeze/thaw lifecycle)
- Overlay disk handling on restore (fresh mkfs vs. stale overlay-template — see m80-f2zc bead set)
- `phase_restore_probe_exec_channel` implementation
- Integration tests against a real Firecracker binary
- Snapshot versioning / compat-check enforcement
- Diff snapshot support (infrastructure is present in the REST types; usage policy is deferred)
