# Preflight Overrides

`m80 preflight` is multi-tenant by default. Overrides are explicit operator
decisions for trusted-tenant, single-tenant, or development hosts where the
default posture is too strict for the current risk model.

Set skip variables only for the invocation that needs them, or place them in a
host-specific operator wrapper. Do not bake skip variables into a production
systemd unit without a written risk acceptance.

## Override Table

| Check | Default posture | Override | Safe only when |
| --- | --- | --- | --- |
| KSM disabled | hard failure when KSM is running | `M80_SKIP_CHECK_KSM=1` | single-tenant host or security signoff accepts page-deduplication side-channel risk |
| SMT disabled | advisory unless escalated | `M80_SKIP_CHECK_SMT=1`; `M80_SMT_CHECK=fail` escalates to hard failure | trusted tenants, or SMT mitigations accepted by the platform owner |
| Swap disabled | hard failure when swap entries are active | `M80_SKIP_CHECK_SWAP=1` | data-remanence risk is accepted and swap policy is documented |
| Nested virtualization disabled | hard failure when KVM nested mode is enabled | `M80_SKIP_CHECK_NESTED_VIRT=1` | trusted host where nested-hypervisor risk is intentionally accepted |
| KVM timer floor | advisory when `min_timer_period_us` is `0` or unavailable | `M80_SKIP_CHECK_KVM_TIMER=1` | timer-interrupt DoS risk is accepted or host tuning is handled outside m80 |
| cgroup favordynmods | advisory on Linux 6.1+ | `M80_SKIP_CHECK_CGROUP_FAVORDYNMODS=1` | cgroup2 `favordynmods` or `kvm.nx_huge_pages=never` posture is documented elsewhere |
| CPU vulnerability rows | hard failure for selected `Vulnerable` sysfs rows | `M80_SKIP_CHECK_VULNERABILITIES=1` | microcode/kernel risk is accepted by the platform owner |

Skip rows are not silent. The relevant check row records `skipped by operator`
or the exact skip variable, so bug reports and JSON output preserve the
operator decision.

## Development Pattern

For a known single-user dev box, keep overrides local to the command:

```sh
M80_SKIP_CHECK_KSM=1 \
M80_SKIP_CHECK_SWAP=1 \
M80_SKIP_CHECK_NESTED_VIRT=1 \
sudo --preserve-env=M80_SKIP_CHECK_KSM,M80_SKIP_CHECK_SWAP,M80_SKIP_CHECK_NESTED_VIRT \
  m80 preflight
```

This is not a production template. Production overrides should be rare,
reviewed, and traceable to a host or fleet risk decision.
