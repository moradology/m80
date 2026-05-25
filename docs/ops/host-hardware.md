# Host Hardware

This is the hardware procurement and admission checklist for production m80
hosts. `m80 preflight` can inspect launch-critical kernel and file-system
state, but it cannot prove every hardware property that matters to a
multi-tenant Firecracker host.

## DRAM

Multi-tenant hosts must use ECC RAM or platform memory with an equivalent
Rowhammer mitigation such as TRR DDR4. m80 assumes the host memory substrate is
appropriate for hostile-tenant isolation; it does not try to detect DRAM type at
launch time.

Non-ECC memory is acceptable only for single-tenant or development hosts where a
bit flip is an intra-tenant reliability problem rather than a cross-tenant
isolation break.

Useful inspection commands:

```sh
edac-util -s
find /sys/bus/platform/drivers/ghes_edac -maxdepth 2 -type f -print
dmidecode -t memory
```

Absence of EDAC reporting is not proof that the host lacks ECC; cloud and
firmware platforms expose this differently. Treat this as procurement evidence,
not an m80 runtime gate.

## CPU And Firmware

The CPU must expose hardware virtualization (`vmx` or `svm`) and firmware must
enable it. Production operators must also keep CPU microcode current. m80's
preflight checks CPU vulnerability sysfs state, but package freshness and early
microcode loading are OS image responsibilities.

For multi-tenant deployments, choose a host class where speculative-execution
mitigations, ECC/TRR memory, and KVM support are all first-order requirements,
not best-effort properties inherited from a generic fleet image.

## m80 Boundary

m80 will not add a DRAM-type preflight check in v0.x. If that changes, the check
must be a best-effort operator signal unless it can be made reliable across bare
metal, cloud, and firmware reporting variants.
