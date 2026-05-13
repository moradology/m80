# CPU Governor Advisory

`m80-preflight` reports the host CPU frequency driver and governor as a
non-blocking `CheckRow` named `CPU governor`.

The advisory is deliberately narrow. When
`/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver` is `acpi-cpufreq` and
`scaling_governor` is anything other than `performance`, the row points
operators at `docs/ops/host-tuning.md` and suggests evaluating:

```sh
sudo cpupower frequency-set -g performance
```

When the driver is `intel_pstate` or `amd_pstate`, the row stays clean even if
the governor is `powersave`, because those drivers use hardware-managed ramp
behavior. Missing cpufreq files are also non-blocking; the row says the check
could not be evaluated.

Tests:

- `crates/m80-preflight/src/checks.rs::tests::cpu_governor_acpi_non_performance_reports_advisory`
- `crates/m80-preflight/src/checks.rs::tests::cpu_governor_acpi_performance_is_clean`
- `crates/m80-preflight/src/checks.rs::tests::cpu_governor_intel_pstate_powersave_is_clean`
- `crates/m80-preflight/src/checks.rs::tests::cpu_governor_amd_pstate_powersave_is_clean`
- `crates/m80-preflight/src/checks.rs::tests::cpu_governor_unavailable_is_non_blocking`
