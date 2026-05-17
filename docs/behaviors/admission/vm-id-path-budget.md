# Admission rejects vm_ids that would overflow the AF_UNIX path cap

## Claim

`Backend::admit(config)` rejects a caller-supplied `vm_id` whose constructed
Firecracker REST API socket path would exceed the kernel's `sun_path` cap of
107 usable bytes. The rejection surfaces as
`FcError::Config(ConfigError::VmIdPathBudgetExceeded { vm_id, run_root,
fc_basename, path_len, budget })` and consumes no admission permit.

This check runs after the single-component shape gate. Values containing path
separators, NULs, unsupported characters, `.` / `..`, or reserved run-root
names fail earlier as `FcError::InvalidVmId`.

## Path layout

`m80-jailer` inherits Firecracker's jailer convention of nesting
`<chroot-base>/<exec-basename>/<id>/root/`. Combined with `m80`'s per-VM
run-dir layout, the socket path is:

```
<run_root>/<vm_id>/<fc_basename>/<vm_id>/root/firecracker.sock
```

`vm_id` appears twice. Each extra byte in `vm_id` therefore costs **two** path
bytes; each extra byte in `run_root` or `fc_basename` costs one.

The arithmetic helper `m80_firecracker::layout::socket_path_len(run_root,
vm_id, fc_basename)` returns the byte length without filesystem access.

## Cap derivation

`struct sockaddr_un` reserves 108 bytes for `sun_path` including the null
terminator. Sockets bound at the kernel cap or above fail with `EINVAL`
("AF_UNIX path too long") at `bind()` / `connect()` time. Usable path bytes
therefore max at 107: `m80_firecracker::layout::SUN_PATH_BUDGET = 107`.

## Worked example

With `run_root = /var/lib/m80-run` (16 bytes) and the standard Firecracker
binary (`firecracker`, basename 11 bytes):

```
total = 16 + 1+V + 1+11 + 1+V + 5 + 17  =  41 + 2V + 11  =  52 + 2V
```

| `vm_id` length V | total path | verdict |
|---|---|---|
| 24 | 100 | admits |
| 27 | 106 | admits (closest fit; vm_id twice means no V hits exactly 107) |
| 28 | 108 | **rejected** with `VmIdPathBudgetExceeded` |
| 36 | 124 | rejected |

A long `fc_basename` (e.g., a synthetic test fake like
`fake-firecracker-cgroup-fail-1234`, 33 bytes) eats further into the budget
and tightens the V boundary correspondingly.

## When the check runs

`admit()` validates a caller-supplied `vm_id` before acquiring the admission
semaphore permit. Auto-generated vm_ids (`vm-{pid}-{ts}`, used when
`SandboxConfig::vm_id` is `None`) are bounded by construction and skip the
check.

A rejected admit returns the typed error and leaves the permit count
unchanged. The caller may retry with a shorter `vm_id` or a shorter
`run_root`.

## Pinned by tests

- `crates/m80-firecracker/src/layout.rs` `tests::socket_path_len_*` - pure
  arithmetic of the path length helper, including the V=27 / V=28 boundary
  and the long-`fc_basename` case.
- `crates/m80-firecracker/tests/path_budget.rs` -
  `admit_within_budget_succeeds`,
  `admit_just_under_budget_succeeds`,
  `admit_over_budget_returns_typed_error`,
  `admit_with_no_vm_id_skips_check`,
  `admit_reserved_run_root_names_fail_before_permit`,
  `admit_over_budget_does_not_consume_permit`.
- `crates/m80-firecracker/tests/security/backend_trust_boundary.rs` -
  `caller_vm_id_shape_fails_before_admission_permit` pins separator, NUL,
  dot-component, empty, bad-character, and over-length rejections as
  `FcError::InvalidVmId`.

## Source

m80-o4z82.1 (real-KVM regression cluster). Replaces an opaque IO error at
launch-time `bind()` with a typed admission-time refusal.
