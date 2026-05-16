# VMGenID Reseed Path

Bead: `m80-q420k.4.10`

The current m80 stripped guest kernel does not expose a guest-observable
VMGenID counter. Firecracker v1.15.1 builds and updates its VMGenID device on
snapshot restore, but the selected m80 kernel profile cannot consume it because
the profile deliberately disables ACPI:

```text
# CONFIG_ACPI is not set
# CONFIG_PCI is not set
# CONFIG_MODULES is not set
```

Linux 6.1.134 names the VMGenID option `CONFIG_VMGENID`, not
`CONFIG_VIRT_GENERATION_ID`. Its Kconfig entry is under `drivers/virt/Kconfig`,
depends on ACPI, and defaults to built-in only when the virtualization-driver
menu and ACPI dependency are available. `drivers/virt/vmgenid.c` consumes the
ACPI IDs `VMGENCTR` / `VM_GEN_COUNTER`; on notification it copies the new
generation ID and calls `add_vmfork_randomness(...)`.

`drivers/char/random.c` handles that call by mixing the unique VM ID into the
random pool and, if the CRNG is already ready, forcing `crng_reseed()` and
logging `crng reseeded due to virtual machine fork`. That path is kernel-side;
there is no stable userspace `generation_counter` ABI in this 6.1 driver.

The Phase 0 PoC matched that source audit. Restoring an erofs+pmem snapshot
kept `/dev/pmem0` and the `dax=always` mount alive, but the guest probe found:

```text
M80_EROFS_POSTCHECK_START time=1778935042840062676 vmgenid=missing
```

So the production hook executor must not block on
`/sys/class/misc/vmgenid/generation_counter` for the current stripped profile.
The current supported path is a host-driven post-restore signal:

1. Host loads the snapshot and resumes the VM.
2. Host sends `PostRestoreHookRequest` over the guest control channel before
   handing the lease to the caller.
3. Guestd performs an explicit userspace reseed step, then runs requested
   typed hooks sequentially.
4. Any reseed or hook failure rejects the lease and tears the VM down.

The userspace reseed step for this profile is:

- call `ioctl(RNDRESEEDCRNG)` on `/dev/urandom`;
- rewrite `/var/lib/systemd/random-seed` if that path exists;
- treat missing systemd random-seed state as normal for minimal/busybox
  guests;
- then run lease-specific hooks such as `RegenMachineId` and `SetHostname`.

`RNDRESEEDCRNG` requires `CAP_SYS_ADMIN`; m80-guestd runs as PID 1/root in the
guest. If the ioctl returns `ENODATA`, the guest CRNG was not ready and the
lease must fail closed.

If m80 later adopts an ACPI-capable snapshot kernel profile with
`CONFIG_VMGENID=y`, the hook executor can switch to a kernel-VMGenID branch:
trust the kernel-side `add_vmfork_randomness` reseed and use the host restore
request only as the sequencing signal. That change needs its own real-KVM
smoke because it changes the stripped-kernel boot profile.

Evidence:

- `crates/m80-image-build/kernel-builder/m80-stripped.config`
- Linux 6.1.134 commit `420102835862f49ec15c545594278dc5d2712f42`,
  `drivers/virt/Kconfig`, `drivers/virt/vmgenid.c`, and
  `drivers/char/random.c`
- `docs/poc/2026-05-16-layered-rootfs-poc-findings.md`
- `/tank/tmp/m80-q420k-poc/fc-pmem-erofs-snapshot-4/`
