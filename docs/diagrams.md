# m80 Diagrams

These diagrams are visual orientation for the main README. The quickstart stays
command-first; this page shows what those commands drive.

## Install Product Shape

`install.sh` selects one published release, verifies it, installs the matched
m80 binary plus VM payloads, and writes the default profile that `m80 run` uses.
KVM, Firecracker, jailer, and seccomp stay host prerequisites and are checked by
preflight.

```mermaid
flowchart TB
    installer["install.sh"]
    release["published m80 release"]
    payloads["matched payloads<br/>m80 binary, guest kernel, rootfs, guestd"]
    proof["checksums, manifests, receipts"]
    install["active m80 install"]
    host["host prerequisites<br/>KVM, Firecracker, jailer, seccomp"]
    run["m80 run"]

    installer --> release
    release --> payloads --> install
    release --> proof --> install
    host -->|"preflight checks"| run
    install -->|"default profile"| run
```

## Process Wrapper Path

`m80 run` wraps one process in one Firecracker microVM and returns stdout,
stderr, and exit status like a normal local command.

```mermaid
flowchart LR
    caller["caller"]
    cli["m80 run"]
    host["host setup<br/>preflight, jailer, cgroup, network, storage"]
    vm["Firecracker microVM"]
    guestd["m80-guestd"]
    proc["wrapped process"]

    caller --> cli --> host --> vm --> guestd --> proc
    proc -->|"stdout, stderr, exit"| cli --> caller
```

## Lifecycle Choices

The CLI exposes cold `m80 run`. Warm restore and persistent VM control are
library or warm-owner surfaces for callers that need lower latency.

```mermaid
flowchart LR
    artifacts["kernel + rootfs + guestd"]
    cold["cold run<br/>boot, exec, teardown"]
    snapshot["snapshot"]
    warm["warm restore<br/>restore, exec, teardown"]
    persistent["persistent VM<br/>boot once, exec repeatedly, stop"]

    artifacts --> cold --> snapshot --> warm
    artifacts --> persistent
```

## Workspace Stack

m80 is split into 23 crates. The stack is intentionally narrow: binaries at the
top, `m80-firecracker` as the orchestration point, and foundation crates below.

```mermaid
flowchart TB
    bins["binaries<br/>m80-cli, m80-image-build, m80-guestd, m80-net-helper"]
    orch["orchestration<br/>m80-firecracker"]
    features["feature crates<br/>net-outbound, snapshot, snapshot-template, observability"]
    foundation["foundation<br/>proto, vsock, preflight, cgroup, jailer, storage,<br/>image-store, image-manifest, firecracker-client, net-mode"]
    tests["test infrastructure<br/>test-helpers, attack-runner, guestd-malicious"]

    bins --> orch
    orch --> features
    orch --> foundation
    tests -. exercise .-> orch
```
