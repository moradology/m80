# `m80-guest-kernel`

Small safe wrappers around the guest-kernel calls that `m80-guestd` needs for
post-restore hooks.

This crate is intentionally outside the main workspace so ordinary m80 crates
keep the workspace-wide `unsafe_code = "forbid"` lint. The raw FFI is isolated
here and exposed as narrow operations:

- `fill_random(buf)` — fill the entire buffer with Linux `getrandom(2)`.
- `set_hostname(name)` — call `sethostname(2)` for a validated hostname.
- `rndreseedcrng(file)` — call `ioctl(RNDRESEEDCRNG)` on `/dev/urandom`.
