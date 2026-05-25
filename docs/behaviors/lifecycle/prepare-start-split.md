# Prepare/Start Split

`Sandbox::prepare()` runs a cold VM through host preparation and Firecracker
preboot REST configuration, then returns `PreparedSandbox` before sending
`InstanceAction::InstanceStart`.

The prepared state owns:

- the admission permit;
- the run directory and ownership lock;
- storage artifacts prepared for the VM;
- the materialized jail and live Firecracker process;
- the optional cgroup subtree and outbound-network residue;
- the Firecracker REST client;
- the inverted-readiness listener at `<vsock.sock>_<READY_PORT_DEFAULT>`;
- launch-failure cleanup guards.

`PreparedSandbox::start()` consumes that handle, sends `InstanceStart`, waits
for the guestd ready signal, runs phase 13 pmem guest mounts when configured,
disarms pre-running cleanup, installs the running drop guard, and returns
`RunningSandbox`.

The Firecracker API socket wait and REST PUT budget are part of `prepare()`,
because phase 11 cannot apply machine, boot-source, drive, entropy, network,
pmem, and vsock configuration without a live API socket. The guest boot and
guest-readiness wait start only after `PreparedSandbox::start()` sends
`InstanceStart`.

`Sandbox::launch()` is the convenience path:

```rust
let running = sandbox.prepare()?.start()?;
```

`PreparedSandbox::abort()` consumes the prepared handle before `InstanceStart`,
lets the pre-running process/network guards tear down host residue, deletes the
run directory, and releases the admission permit.

This split does not add a paused guest state. The guest has not started while
the handle is `PreparedSandbox`; only host and VMM setup have completed.

## Verification

- `crates/m80-firecracker/tests/prepare_start.rs::prepare_start_yields_functional_running_sandbox`
  proves `prepare()?.start()` reaches a usable `RunningSandbox` on real KVM.
- `crates/m80-firecracker/tests/prepare_start.rs::prepared_abort_releases_permit_and_deletes_run_dir`
  proves abort releases admission and deletes the prepared run directory.
- `crates/m80-firecracker/tests/prepare_start.rs::prepared_sandbox_can_start_after_delay`
  proves the host can hold a prepared VM before sending `InstanceStart`.
- `crates/m80-firecracker/tests/prepare_start.rs::launch_convenience_path_still_yields_functional_running_sandbox`
  proves `launch()` remains a functional convenience path.
