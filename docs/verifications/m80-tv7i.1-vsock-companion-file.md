# Verification: does Firecracker create `<vsock_uds>_<port>` on guest `bind()`?

**Bead**: m80-tv7i.1 (gating leaf for m80-tv7i — Marker-file readiness probe)
**Date**: 2026-05-04
**Result**: **No. Premise refuted. Bead m80-tv7i closes with `--reason "premise-refuted"`.**

## Summary

The bead m80-tv7i hypothesized that Firecracker would create a host-side
companion file `<vsock_uds>_<port>` (e.g., `vsock.sock_9001`) when an
in-VM consumer called `VsockListener::bind((VMADDR_CID_ANY, 9001))`,
allowing the host to detect guest readiness via cheap `stat()` polling.

Source-code reading and empirical observation both refute this. The
`<uds>_<port>` companion file is:

1. **Created by the host**, not by Firecracker;
2. **Used for guest-initiated outbound connections** (guest → host port),
   not for advertising guest-side listeners;
3. **Never materialized on the host side by Firecracker itself**.

## Evidence

### Source-code reading — Firecracker v1.15.1

`/tank/projects/firecracker/src/vmm/src/devices/virtio/vsock/unix/muxer.rs:99-104`

```rust
host_sock: UnixListener,                                         // 99
…                                                                // 100-101
/// The file system path of the host-side Unix socket. This is used to figure out the path
/// to Unix sockets listening on specific ports. I.e. `"<this path>_<port number>"`.
pub(crate) host_sock_path: String,                               // 102
```

The doc-comment says `<this path>_<port number>` is a path **listening
elsewhere**, not something this struct creates. The struct only owns
`host_sock` (singular) — the main muxer UDS.

`/tank/projects/firecracker/src/vmm/src/devices/virtio/vsock/unix/muxer.rs:619-641`

```rust
/// Handle a new connection request comming from our peer (the guest vsock driver).
///
/// This will attempt to connect to a host-side Unix socket, expected to be listening at
/// the file system path corresponing to the destination port. If successful, a new
/// connection object will be created and added to the connection pool. On failure, a new
/// RST packet will be scheduled for delivery to the guest.
fn handle_peer_request_pkt(&mut self, pkt: &VsockPacketTx) {
    let port_path = format!("{}_{}", self.host_sock_path, pkt.hdr.dst_port());

    UnixStream::connect(port_path)
        .and_then(|stream| stream.set_nonblocking(true).map(|_| stream))
        .map_err(VsockUnixBackendError::UnixConnect)
        …
        .unwrap_or_else(|_| self.enq_rst(pkt.hdr.dst_port(), pkt.hdr.src_port()));
}
```

This is the **only** call site that constructs `<host_sock_path>_<port>`.
It does `UnixStream::connect()` — **the host must already be listening**.
If the host isn't listening, Firecracker `enq_rst()`s back to the guest.

### Source-code reading — search exhaustiveness

```
$ grep -rn "host_sock_path" /tank/projects/firecracker/src/vmm/src/devices/virtio/vsock/unix/
muxer.rs:102:    pub(crate) host_sock_path: String,
muxer.rs:312:    pub fn new(cid: u64, host_sock_path: String) -> …
muxer.rs:315:        let host_sock = UnixListener::bind(&host_sock_path) …
muxer.rs:322:            host_sock_path,
muxer.rs:338:    pub fn host_sock_path(&self) -> &str { … }
muxer.rs:620:        let port_path = format!("{}_{}", self.host_sock_path, pkt.hdr.dst_port());
muxer.rs:824:        std::fs::remove_file(self.muxer.host_sock_path.as_str()).unwrap();   # tests
muxer.rs:923:        LocalListener::new(format!("{}_{}", self.muxer.host_sock_path, port)) # tests
muxer.rs:929:        let mut stream = UnixStream::connect(self.muxer.host_sock_path.clone()).unwrap();
```

Every non-test reference treats `<host_sock_path>_<port>` as a host
listener path. The only `UnixListener::bind` call against `host_sock_path`
itself is line 315 — for the singular muxer UDS, not per-port files.
There is no Firecracker code path that creates per-port companion files.

### Empirical — m80 smoke test on KVM

Launched a real VM with `m80 launch --network noegress` (which configures
vsock at `<jail>/vsock.sock`). m80-guestd binds vsock port 9001 inside
the VM at startup. Sampled `<jail>/vsock.sock*` paths across the full
lifecycle:

```
T+1s   no vsock.sock* files yet
T+3s   no vsock.sock* files yet
T+6s   /var/lib/m80-run/<vm>/firecracker/<vm>/root/vsock.sock     0 bytes  socket
T+10s  /var/lib/m80-run/<vm>/firecracker/<vm>/root/vsock.sock     (same)
T+15s  /var/lib/m80-run/<vm>/firecracker/<vm>/root/vsock.sock     (same)
T+25s  /var/lib/m80-run/<vm>/firecracker/<vm>/root/vsock.sock     (same)
final  srwxr-xr-x 1 nathan kvm 0 May 4 13:29 …/vsock.sock
```

Only the singular muxer UDS appears. **No `vsock.sock_9001` ever
materializes**, even after m80-guestd has fully started, bound port 9001,
and is serving requests over it. The CONNECT/OK handshake we use today
(via the singular `vsock.sock`) is what `phase_12b_ready_probe`
implicitly demonstrates — the host writes `CONNECT 9001\n` to the muxer
UDS, the muxer routes to the guest's listener, and replies `OK`.

### Cross-check — smolvm

Per `/tank/projects/m80/smolvm-exploration/03-boot-path-and-readiness.md`,
smolvm uses a virtiofs marker file (`.smolvm-ready`) written by the
in-VM agent immediately after `vsock::listen()` succeeds. The host
stat-polls the marker via virtiofs.

Smolvm's choice corroborates the verification: a cooperative real-world
consumer of microVM vsock chose **not** to rely on a host-side companion
file as a readiness signal. They went to a different file system
(virtiofs) to get the marker.

## Implications for the m80-tv7i family

Per the bead's own acceptance criteria, this verification triggers the
"premise refuted" branch:

1. **m80-tv7i.1** closes with `--reason "premise-refuted"` (this leaf).
2. **m80-tv7i** (parent) closes with the same reason. Its other leaves
   (.2 IMPL, .3 TEST-UNIT, .4 TEST-INT, .5 DOCS) close as superseded.
3. **m80-7tpy** (inverted readiness — guest connects out, host
   `accept()`s on `<uds>_<ready_port>`) is **strengthened** by this
   verification. The mechanism it relies on (Firecracker calls
   `UnixStream::connect("<uds>_<port>")` when the guest does outbound
   vsock to that port) is exactly what the source code shows. m80-7tpy
   becomes the primary event-driven readiness path.
4. **m80-bgas** (parity quick-wins: 100ms poll, 60s timeout) is the
   correct near-term backstop and remains unchanged.
5. **m80-6a0q** (minimal-init / guestd as PID 1) is unaffected; it
   addresses the orthogonal cold-boot latency floor.
6. The bead's contingency to "file a new bead under m80-6gv9 to
   investigate the `PUT /logger` serial-console-marker path" remains
   open as a separate option, but is now lower priority than m80-7tpy.

## Files cited

- `/tank/projects/firecracker/src/vmm/src/devices/virtio/vsock/unix/muxer.rs:99-104, 312-322, 619-641`
- `/tank/projects/m80/crates/m80-firecracker/src/launch.rs:phase_12b_ready_probe`
  (current poll-and-CONNECT/OK probe)
- `/tank/projects/m80/crates/m80-guestd/src/main.rs::run` (vsock bind)
- `/tank/projects/m80/scripts/smoke.sh` (verification harness)
- `/tank/projects/m80/smolvm-exploration/03-boot-path-and-readiness.md`
  (cross-check)
