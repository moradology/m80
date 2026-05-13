# `m80-close-range`

Small safe wrapper around Linux `close_range(2)` for `m80-jailer-harden`.

## Contract

`close_from(min_fd)` closes every file descriptor from `min_fd` through
`UINT_MAX` with flags `0`. It returns the kernel error if the syscall fails.

This crate is intentionally outside the main m80 workspace so ordinary m80
crates can keep the workspace-wide `unsafe_code = "forbid"` lint. The raw FFI
is isolated here and exposed as one narrow, typed operation.
