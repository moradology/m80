# Post-Restore Reseed

## Behavior

Every successful snapshot restore performs a guest-side reseed gate before
`m80-firecracker` returns the restored `RunningSandbox`.

The host sequence is:

1. Firecracker loads and resumes the snapshot.
2. m80 proves the restored guestd exec channel with the internal readiness
   probe.
3. m80 sends `PostRestoreHookRequest` with a fresh 32-byte host nonce.
4. guestd writes that nonce to `/dev/urandom`, calls `RNDRESEEDCRNG`, runs the
   ordered closed hook list, and replies with `PostRestoreHookResponse`.

Plain `Sandbox::launch_from_snapshot` sends an empty hook list. That still runs
the nonce mix and `RNDRESEEDCRNG`; it only skips optional identity hooks such as
machine-id and hostname rewrite.

## Residual Gap

The reseed gate refreshes the guest kernel CRNG after restore. It does not
rewind userspace PRNGs that already buffered random bytes before capture, and
it does not perform image-specific work such as SSH host-key regeneration or
daemon restarts. Those actions require new typed `HookSpec` variants.

`ReseedSystemdRandomSeed` rewrites `/var/lib/systemd/random-seed` when present.
Missing systemd random-seed state is normal for minimal and non-systemd guests
and does not weaken the baseline nonce mix plus `RNDRESEEDCRNG` path.

## Tests

- `crates/m80-guestd/src/post_restore/tests.rs` pins that a restore request
  with an empty hook list still calls the reseed operation.
- `crates/m80-firecracker/src/lifecycle/post_restore.rs` pins that the host
  still sends a `PostRestoreHookRequest` for an empty `HookSpecSet`.
- `crates/m80-firecracker/tests/snapshot_integration.rs` contains
  `plain_restore_reseeds_urandom_before_handoff`, an ignored real-KVM restore
  proof that two restores from the same snapshot receive distinct post-restore
  `/dev/urandom` samples before hand-back.
