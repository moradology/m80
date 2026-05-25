# Threat Model

m80 relies on Firecracker's isolation boundary, the Linux KVM boundary, m80's
jailer setup, and host operator posture. This document records the operator
part of that boundary for production hosts.

## What Firecracker Provides

Each m80 sandbox is a KVM microVM with its own guest kernel and Firecracker VMM
process. The guest process runs inside that VM; m80 does not treat guest code as
trusted. Host-side isolation comes from KVM, Firecracker's device model,
mandatory jailer confinement, cgroups, namespaces, seccomp, and m80's launch
validation.

Firecracker's security policy is upstream; m80's job is to configure and wrap
that boundary without adding hidden agent policy to the VM-mechanics layer.

## What Operators Provide

Production operators are responsible for:

- current CPU microcode and kernel mitigations,
- acceptable CPU vulnerability sysfs state,
- SMT policy appropriate to the tenant model,
- ECC or TRR-equivalent memory for multi-tenant hosts,
- no nested virtualization unless explicitly accepted for a trusted host,
- artifact and binary ownership described in [`host-setup.md`](host-setup.md).

m80 preflight hard-fails selected CPU vulnerability rows and provides explicit
skip variables for operators who accept the risk. m80 does not read every vendor
mitigation detail or decide what an Intel, AMD, or ARM advisory means for a
specific fleet.

## Speculative Execution

For Spectre/Meltdown-class issues, defer to CPU vendor and distribution
guidance for the exact mitigation set. m80 records the host-facing gate, not a
complete vulnerability scanner.

For L1TF and MDS-class issues, disabling SMT is the cleanest multi-tenant
posture. If SMT remains enabled, either keep it advisory-only for trusted
tenants or set:

```sh
M80_SMT_CHECK=fail
```

to make the SMT row a hard preflight failure. Operators can set
`M80_SKIP_CHECK_SMT=1` only after accepting the risk.

## Scope

This document covers host-to-guest and host-to-host isolation posture. It does
not define adapter authorization, workspace authority, tool catalogs, or
semantic event policy. Those live above m80 per
[`docs/adapter-boundary.md`](../adapter-boundary.md).
