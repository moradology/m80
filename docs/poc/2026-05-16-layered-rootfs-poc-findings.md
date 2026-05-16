# Layered rootfs PoC findings

Date: 2026-05-16

Scratch root: `/tank/tmp/m80-q420k-poc`

Substrate:

- Host kernel: `Linux vulcan 6.17.0-23-generic x86_64`
- Firecracker: `/opt/firecracker/bin/firecracker`, `v1.15.1`
- Guest kernel/rootfs for direct Firecracker runs:
  `/tmp/m80-build/ubuntu/vmlinux` and disposable copy of
  `/tmp/m80-build/ubuntu/output.ext4`
- Stripped-kernel cross-check:
  `crates/m80-image-build/kernels/vmlinux-m80-a096548ede447f98a883c9d0a23b5007f721b2911233186465559f3553992ed0.bin`
- Rebuilt stripped kernel after the pmem/DAX config fix:
  `crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin`
- Toolchain source: `/home/nathan/.rustup/toolchains/1.82-x86_64-unknown-linux-gnu`

## PoC-1: erofs toolchain image

Command shape:

```sh
mkfs.erofs -zlz4hc,level=9 -T0 --all-root \
  /tank/tmp/m80-q420k-poc/rust-toolchain-1.82.erofs \
  /home/nathan/.rustup/toolchains/1.82-x86_64-unknown-linux-gnu
```

Observed:

- Input bytes: `765386417`
- Output bytes: `366387200`
- Build time: `1.34s`
- Image sha256:
  `2bf1023c449d66ef6908af579f846a11d1e238ba6148aadf4b5ce61568729214`
- Host loop mount succeeded:
  `erofs ro,relatime,user_xattr,acl,cache_strategy=readaround`
- Host-side binaries from the mounted image worked:
  `cargo 1.82.0`, `rustc 1.82.0`

## PoC-2: erofs over pmem

Direct Firecracker API calls all succeeded:

- `PUT /machine-config`: `204`
- `PUT /boot-source`: `204`
- `PUT /drives/rootfs`: `204`
- `PUT /pmem/pmem0`: `204`
- `PUT /actions`: `204`

The guest saw the pmem device:

```text
brw------- 1 root root 259, 0 May 16 12:03 /dev/pmem0
```

But mounting erofs failed in the guest:

```text
mount: /opt/toolchain: unknown filesystem type 'erofs'.
M80_POC mount-exit:32
```

Finding: the current m80 Ubuntu guest kernel/rootfs substrate used for this
PoC has pmem/DAX support, but does not support erofs in-guest. This blocks the
planned Phase B `erofs + virtio-pmem + DAX` path until kernel config / module
availability is made explicit and pinned by smoke.

### Stripped-kernel cross-check

The newest committed stripped kernel was then booted with the same disposable
Ubuntu rootfs and the same erofs pmem image. Firecracker API setup still
returned `204` for machine, boot, root drive, pmem, and start. The guest booted
Linux `6.1.134` and reported erofs support:

```text
M80_STRIPPED uname Linux (none) 6.1.134 #1 PREEMPT_DYNAMIC 0 x86_64 ...
M80_STRIPPED pmem-list ls: cannot access '/dev/pmem*': No such file or directory
M80_STRIPPED filesystems     ext4;   erofs;
erofs: dax options not supported
M80_STRIPPED mount-dax-exit:32
/dev/pmem0: Can't open blockdev
M80_STRIPPED mount-ro-exit:32
```

Finding: the stripped kernel has erofs built in, but this build did not expose
the Firecracker pmem device and rejected the erofs `dax` option. The viable
Phase B path needs one pinned guest kernel profile that has erofs,
virtio-pmem/block-pmem, and the DAX form that the guest mount handler will
assert.

### Rebuilt stripped-kernel follow-up

`m80-stripped.config` was updated to keep virtio-pmem, libnvdimm, block pmem,
filesystem DAX, and the required zone-device memory model built in. The
rebuilt kernel path was:

```text
crates/m80-image-build/kernels/vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin
```

A direct Firecracker rerun with the original erofs toolchain image then
succeeded:

```text
M80_VERIFY uname Linux (none) 6.1.134-g420102835862 #1 PREEMPT_DYNAMIC 0 x86_64 ...
M80_VERIFY pmem-list brw------- 1 root root 259, 0 May 16 12:26 /dev/pmem0
M80_VERIFY filesystems     ext4;   erofs;
M80_VERIFY pmem0-queue-dax=1
M80_VERIFY mount-dax-exit:0
M80_VERIFY mounts /dev/pmem0 /opt/toolchain erofs ro,relatime,user_xattr,acl,cache_strategy=readaround,dax=always 0 0
M80_VERIFY cargo cargo 1.82.0 (8f40fc59f 2024-08-21)
M80_VERIFY cargo-exit:0
M80_VERIFY rustc rustc 1.82.0 (f6e511eec 2024-10-15)
M80_VERIFY rustc-exit:0
M80_VERIFY_DONE mount_rc=0 cargo_rc=0 rustc_rc=0
```

Finding: the selected stripped kernel profile now supports the Phase B
`erofs + virtio-pmem + dax=always` mount shape on real KVM. The proof is a
kernel capability smoke, not a memory-sharing measurement.

A final timed direct boot used the same kernel and erofs image with run
artifacts at `/tank/tmp/m80-q420k-poc/fc-pmem-erofs-timed-boot`:

```text
M80_TIMED mount-dax-exit:0
M80_TIMED mounts /dev/pmem0 /opt/toolchain erofs ro,relatime,user_xattr,acl,cache_strategy=readaround,dax=always 0 0
M80_TIMED cargo cargo 1.82.0 (8f40fc59f 2024-08-21)
M80_TIMED cargo-exit:0 cargo_ns:29804884
M80_TIMED rustc rustc 1.82.0 (f6e511eec 2024-10-15)
M80_TIMED rustc-exit:0 rustc_ns:66888086
M80_TIMED_DONE time_ns=1778935179195711369 mount_rc=0 cargo_rc=0 rustc_rc=0
```

Host-side start-to-done time for that scratch harness was `2036 ms`; first
`cargo --version` inside the guest took `29.8 ms`.

## Ext4 detour: pmem and DAX isolation

To keep testing pmem and snapshot behavior independently from erofs support,
the same toolchain was copied into a disposable ext4 image:

- Output path: `/tank/tmp/m80-q420k-poc/rust-toolchain-1.82.ext4`
- Output bytes: `2147483648`
- Image sha256:
  `31a6b122029c03a2550740363da4704b054b5303ec41197ecd4275bf63c1186c`

Guest mount and execution succeeded:

```text
/dev/pmem0 /opt/toolchain ext4 ro,relatime,norecovery,dax=always 0 0
cargo 1.82.0 (8f40fc59f 2024-08-21)
M80_POC_EXT4 cargo-exit:0
rustc 1.82.0 (f6e511eec 2024-10-15)
M80_POC_EXT4 rustc-exit:0
```

Finding: Firecracker pmem attach, guest `/dev/pmem0`, DAX mount, and executing
the Rust toolchain from the pmem-backed filesystem all work on this substrate
when the guest filesystem is ext4.

## PoC-3/4: snapshot plus pmem composition

Using the ext4 pmem detour, full snapshot create and restore succeeded:

- Initial run:
  - `PATCH /vm`: `204`
  - `PUT /snapshot/create`: `204`
- Restored run:
  - `PUT /snapshot/load`: `204`
- Snapshot files:
  - `vmstate.snap`: `14065` bytes
  - `mem.snap`: `1073741824` bytes

Firecracker logs show VMGenID changed and notified on restore:

```text
vmgenid: writing new generation ID to guest: 0xd2518428e68c9805088d9c073558dd41
vmgenid: writing new generation ID to guest: 0xacadd328f3cfc28704d8f9e546427066
vmgenid: notifying guest about new generation ID
kick pmem pmem0.
'load snapshot' VMM action took 2125 us.
```

The restored guest still had the pmem device and the already-mounted DAX
filesystem, and `cargo` worked without re-mounting:

```text
M80_POSTCHECK_START time=1778933288781715056 vmgenid=missing
brw------- 1 root root 259, 0 May 16 12:07 /dev/pmem0
/dev/pmem0 /opt/toolchain ext4 ro,relatime,norecovery,dax=always 0 0
cargo 1.82.0 (8f40fc59f 2024-08-21)
M80_POST post-cargo-exit:0
```

A minimal post-restore hook shape also worked once run explicitly by the PoC
script after restore:

```text
M80_POST machine-id-old 6acd482830ec4ce6b52a0d7c7eefbfff
M80_POST machine-id-post 76d707f44b3a2c18d0cd45967fc3844a
M80_POST hostname-post m80-poc-restored
```

Findings:

- Firecracker snapshot/restore preserves and re-kicks a pmem device on this
  substrate when the backing file remains at the same host path.
- A filesystem mounted from pmem before snapshot is still mounted after
  restore.
- Firecracker changed and notified VMGenID, but the guest userspace probe found
  no `vmgenid` or `generation_counter` sysfs file. Phase D must not assume the
  bead body's example path `/sys/class/misc/vmgenid/generation_counter` exists
  for m80's guest kernel/rootfs.

### Erofs snapshot composition follow-up

After rebuilding the stripped kernel with pmem/DAX support, the PoC repeated
snapshot/restore with the original erofs image instead of the ext4 detour.
Run directory:
`/tank/tmp/m80-q420k-poc/fc-pmem-erofs-snapshot-4`.

API calls succeeded:

```text
machine 204
boot 204
drive 204
pmem 204
start 204
pause 204
snapshot 204
load 204
```

Snapshot files:

- `vmstate.snap`: `14183` bytes
- `mem.snap`: `1073741824` bytes

The restored guest continued from the snapshotted process and still had the
pmem-backed erofs DAX mount:

```text
M80_EROFS_POSTCHECK_START time=1778935042840062676 vmgenid=missing
M80_EROFS_POST pmem-list brw------- 1 root root 259, 0 May 16 12:36 /dev/pmem0
M80_EROFS_POST mounts /dev/pmem0 /opt/toolchain erofs ro,relatime,user_xattr,acl,cache_strategy=readaround,dax=always 0 0
M80_EROFS_POST cargo cargo 1.82.0 (8f40fc59f 2024-08-21)
M80_EROFS_POST cargo-exit:0
M80_EROFS_POST machine-id-old 76d707f44b3a2c18d0cd45967fc3844a
M80_EROFS_POST machine-id-post a4e7dacebee946fd90f29871ecc8fe2c
M80_EROFS_POST hostname-post m80-erofs-restored
M80_EROFS_POSTCHECK_DONE time=1778935042914081975 post_cargo_rc=0
```

Measured harness timing from this scratch script:

- Ready-marker wait after start: `2022 ms`
- Pause API call: `13 ms`
- Snapshot API call: `545 ms`
- Load API response: `13 ms`
- Load-to-postcheck marker: `30209 ms`

The load-to-postcheck value is dominated by the deliberate 30 second guest
sleep inserted to avoid racing the snapshot. It is not a restore-latency
measurement.

Finding: Firecracker v1.15.1 preserves the pmem device and already-mounted
erofs DAX filesystem across snapshot/restore when the backing file path is
stable. VMGenID is still not guest-observable through the probed sysfs paths.

## PoC-5: calibration only

Four concurrent guests were booted with separate disposable rootfs copies and
the same erofs pmem backing file:
`/tank/tmp/m80-q420k-poc/rust-toolchain-1.82.erofs`.

Run directory: `/tank/tmp/m80-q420k-poc/fc-pmem-erofs-n4`.

All four guests mounted the same erofs file through pmem and reached the ready
marker:

```text
vm1 M80_N4_READY time=1778935245626158849 mount_rc=0 cargo_rc=0
vm2 M80_N4_READY time=1778935245927654447 mount_rc=0 cargo_rc=0
vm3 M80_N4_READY time=1778935246214790266 mount_rc=0 cargo_rc=0
vm4 M80_N4_READY time=1778935246481876992 mount_rc=0 cargo_rc=0
```

Host memory snapshot:

- erofs image size: `366387200` bytes
- baseline `MemAvailable`: `126764516 KiB`
- after N=4 ready `MemAvailable`: `126407656 KiB`
- after teardown `MemAvailable`: `126667672 KiB`
- baseline-to-ready delta: `356860 KiB`
- baseline-to-post-teardown delta: `96844 KiB`

Ten restore API calls were then run from the erofs snapshot artifact:
`/tank/tmp/m80-q420k-poc/fc-pmem-erofs-restore-n10`.

```text
1  204  14 ms
2  204  16 ms
3  204  15 ms
4  204  15 ms
5  204  14 ms
6  204  13 ms
7  204  14 ms
8  204  12 ms
9  204  14 ms
10 204  15 ms
```

Summary:

- all load API calls returned `204`
- p50 load API response: `14 ms`
- p95 load API response: `16 ms`
- p99 load API response: `16 ms`

Finding: the calibration run did not show a 4x erofs-size host memory delta
for four guests sharing one erofs pmem backing file, and snapshot load API
latency was low on this host. These are scratch calibration numbers only; they
are not a production perf artifact and do not prove file-level DAX page sharing.

## Back-propagation required

- Phase B must depend on the explicit stripped-kernel support gate for the
  combined erofs + virtio-pmem/block-pmem + DAX profile before implementing
  `PmemLayer { sharing: PerVm }` as erofs-only.
- Phase B guestd mount work must treat `dax` spelling and filesystem support as
  real-KVM-discovered substrate facts. On the rebuilt stripped kernel,
  `dax=always` appears in `/proc/mounts`.
- Phase D VMGenID work must audit the actual m80 guest kernel and choose a
  guest-observable reseed trigger. Firecracker logs alone are not a sufficient
  guest-side hook trigger.
- Phase D snapshot-template work may assume pmem device state is restorable only
  with the backing path stable; this was observed directly.
- Phase C/F measurement work must prove actual host memory behavior. This PoC
  used a compressed erofs image and did not measure page-cache sharing or
  file-level DAX behavior across guests.

## Not completed

- No production measurement artifact was produced. PoC-5 numbers above are
  scratch calibration only and should not be used to close Phase F
  `requires-verified-close` beads.
