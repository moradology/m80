# Guest Uevent Hotplug Infrastructure

Behavior captured by bead `m80-iswt.4`.

## Present-tense statement

`m80-guestd` has a guest-local Linux uevent infrastructure for future drive
hotplug handlers. The layer parses kernel `NETLINK_KOBJECT_UEVENT` messages,
stores parsed events in a cache, and lets callers wait for matching devices
with a bounded `Condvar` wait.

## Contract

- `parse_uevent` accepts the kernel's NUL-delimited uevent message shape:
  `action@devpath`, followed by `KEY=value` properties.
- `BlockDeviceMatcher` matches `add` and `change` events whose subsystem is
  `block`; callers can match any block device or a specific `DEVNAME`.
- `UeventRegistry::record` caches every parsed event and wakes blocked waiters.
- `UeventRegistry::wait_for` first checks the cache, then waits on a condition
  variable until a matching event arrives or the caller's timeout expires.
- The netlink listener is infrastructure only at this stage. It does not mount
  drives by itself.

## Tests

- `crates/m80-guestd/src/uevent.rs` unit tests cover parser fields, matcher
  filtering, cached-event return, blocking wait wakeup, and timeout behavior.
