# Post-Restore Hooks

This behavior follows the current-profile decision in
[`docs/behaviors/warm-pool/vmgenid-reseed-path.md`](vmgenid-reseed-path.md):
the stripped guest kernel does not expose guest-visible VMGenID, so v0.1 uses a
host-generated restore nonce plus guestd's `RNDRESEEDCRNG` call as the
restore-sequenced reseed path.

Post-restore hooks are the host-driven lease-handoff gate for snapshot-template
warm slots. After Firecracker loads and resumes a snapshot, `m80-firecracker`
first proves the restored guestd can execute an internal readiness probe. If a
hook set is configured, it then sends one `PostRestoreHookRequest` over the
restored vsock channel and waits for `PostRestoreHookResponse` before returning
the `RunningSandbox` or making a warm slot ready.

The request carries a fresh 32-byte host restore nonce. On the current stripped
kernel profile, guest userspace cannot observe Firecracker VMGenID, so guestd
treats the host request as the restore sequencing signal. Guestd writes the
nonce to `/dev/urandom`, calls `RNDRESEEDCRNG`, then runs the ordered closed
hook list. A nonce-mix or reseed failure is `HookError::ReseedFailed` and
aborts lease handoff.

The closed hook vocabulary is:

- `ReseedSystemdRandomSeed`: rewrite `/var/lib/systemd/random-seed` with fresh
  guest random bytes when that path exists; absence is allowed for minimal
  guests and does not create parent directories.
- `RegenMachineId`: rewrite `/etc/machine-id` from fresh guest random bytes.
- `SetHostname`: validate the hostname again in guestd, call `sethostname(2)`,
  and write `/etc/hostname`.

Non-systemd and busybox-style guests are supported by the baseline nonce mix
and `RNDRESEEDCRNG` path. Missing `/var/lib/systemd/random-seed` is normal,
and an empty hook list still performs the nonce mix and kernel reseed before
guestd responds. m80 does not infer image-specific uniqueness work such as SSH
host-key regeneration, daemon restart, or application PRNG cache flushing. If
a real guest profile needs one of those actions, it must become a new closed
`HookSpec` variant rather than a caller-provided shell command.

Hook execution is sequential and fail-closed. The first failed hook returns one
typed `HookError`, no later hook runs, and `m80-firecracker` maps that response
to `FcError::PostRestoreHook`. Successful restore APIs return only after every
requested hook has acknowledged success.

The host enforces one aggregate 5 second response deadline for the
`PostRestoreHookRequest`. That deadline covers nonce mixing, `RNDRESEEDCRNG`,
and every requested hook. If guestd hangs or does not return a complete
`PostRestoreHookResponse` before the deadline, `m80-firecracker` treats the
handoff as a protocol read timeout, tears down the restored VM through the
normal restore-failure cleanup path, and does not expose the lease.

## Evidence

- `crates/m80-proto/tests/post_restore_hooks_round_trip.rs` covers the closed
  wire shape.
- `crates/m80-guestd/src/post_restore/` owns the guest-side nonce mix, reseed,
  and hook executor, including the empty-hook-list reseed case.
- `crates/m80-firecracker/src/lifecycle/post_restore.rs` covers hook ordering,
  response validation, request-id matching, typed hook failure mapping, and the
  host-side aggregate response timeout.
- Real-KVM scenario tests for reseed, machine-id, hostname, and hook-failure
  behavior are tracked in `m80-q420k.4.13`.
