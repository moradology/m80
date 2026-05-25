# Max Lifetime

Behavior capture for `m80-2ggw.7.1`.

## Contract

`SandboxConfig::max_lifetime: Option<Duration>` is an absolute wall-clock cap
on a running VM. The default is `None`, so existing callers keep running until
they stop, force-kill, drop, or hit another configured lifecycle guard.

When configured, the deadline starts when cold launch or snapshot restore
returns `RunningSandbox`. The deadline does not reset after exec, file-op,
ping, guest metrics, or hotplug activity. That makes it independent of
`SandboxConfig::idle_timeout`, which is based on time since the last accepted
work.

The lifecycle watcher sets a `lifetime_expired` flag once the deadline elapses.
If no request is in flight, it also sends the same best-effort graceful
shutdown request used by idle timeout. If a request is already in flight, that
request is allowed to finish; the next lifecycle operation returns
`FcError::LifetimeExpired { limit }`.

When `idle_timeout` and `max_lifetime` are both configured, the first deadline
to expire wins and determines the typed error reported to the caller.

## CLI Surface

`m80-cli` maps `FcError::LifetimeExpired` to exit code 9 and emits
`"LifetimeExpired"` in JSON error envelopes.

## Verification

- `crates/m80-firecracker/src/lifecycle.rs` unit tests pin watcher behavior
  for in-flight lifetime expiry and idle-before-lifetime ordering.
- `crates/m80-firecracker/tests/max_lifetime.rs` pins the public default,
  typed error display, and real-KVM ignored coverage for no-exec expiry,
  in-flight exec completion before the next rejection, `None` opt-out, and
  idle-vs-lifetime winner behavior.
- `crates/m80-cli/src/errors.rs` tests the CLI exit-code and JSON variant
  mapping.
