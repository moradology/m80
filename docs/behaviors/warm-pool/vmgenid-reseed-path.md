# VMGenID Reseed Path

Bead: `m80-q420k.4.10`

Decision: `m80-q420k.4.16`

m80 v0.1 does **not** add a second ACPI/PCI-capable snapshot kernel profile
for VMGenID. Phase D proceeds with the current stripped kernel profile and the
host-driven post-restore reseed path below: the host sends a fresh
`PostRestoreHookRequest.restore_nonce`, guestd mixes it into `/dev/urandom`,
and guestd calls `RNDRESEEDCRNG` before lease-specific hooks. This keeps the
snapshot-template path on the same legacy virtio-mmio boot profile as the rest
of v0.1 and avoids expanding the kernel surface before the template restore
contract is proven.

An ACPI/`CONFIG_VMGENID=y` profile remains a future, separate kernel-profile
change. It must not be assumed by current Phase D code, and if adopted later it
needs its own real-KVM smoke proving cold boot, snapshot restore, and the Linux
VMGenID reseed path.

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
2. Host generates a fresh 32-byte restore nonce with `getrandom()`.
3. Host sends `PostRestoreHookRequest { restore_nonce, hooks }` over the
   guest control channel before handing the lease to the caller.
4. Guestd mixes the nonce into the guest random pool, performs an explicit
   userspace reseed step, then runs requested typed hooks sequentially.
5. Any reseed or hook failure rejects the lease and tears the VM down.

The userspace reseed step for this profile is:

- write the host-generated restore nonce to `/dev/urandom` so clones do not
  ask identical snapshotted entropy pools to reseed without fresh input;
- call `ioctl(RNDRESEEDCRNG)` on `/dev/urandom`;
- rewrite `/var/lib/systemd/random-seed` if that path exists;
- treat missing systemd random-seed state as normal for minimal/busybox
  guests;
- then run lease-specific hooks such as `RegenMachineId` and `SetHostname`.

For non-systemd guests, the guaranteed m80-owned uniqueness step is the
restore nonce plus `RNDRESEEDCRNG`; it runs even when the requested hook list
is empty. `ReseedSystemdRandomSeed` is an optional file rewrite, not the
definition of reseed success. m80 also owns generic guest identity hooks such
as `/etc/machine-id` and hostname. It does not regenerate SSH host keys,
restart image-specific daemons, or flush application-level PRNG caches in
v0.1 because those are image/application semantics, not VM mechanics. A future
caller that needs one of those actions must add a new closed `HookSpec`
variant with typed validation and tests; arbitrary post-restore shell commands
remain rejected.

`RNDRESEEDCRNG` requires `CAP_SYS_ADMIN`; m80-guestd runs as PID 1/root in the
guest. If the ioctl returns `ENODATA`, the guest CRNG was not ready and the
lease must fail closed.

If m80 later adopts an ACPI-capable snapshot kernel profile with
`CONFIG_VMGENID=y`, the hook executor can switch to a kernel-VMGenID branch:
trust the kernel-side `add_vmfork_randomness` reseed and use the host restore
request only as the sequencing signal. That change needs its own real-KVM
smoke because it changes the stripped-kernel boot profile.

Evidence:

- `docs/decisions/0007-snapshot-template-lifecycle.md`
- `crates/m80-image-build/kernel-builder/m80-stripped.config`
- Linux 6.1.134 commit `420102835862f49ec15c545594278dc5d2712f42`,
  `drivers/virt/Kconfig`, `drivers/virt/vmgenid.c`, and
  `drivers/char/random.c`
- `docs/poc/2026-05-16-layered-rootfs-poc-findings.md`
- `/tank/tmp/m80-q420k-poc/fc-pmem-erofs-snapshot-4/`
