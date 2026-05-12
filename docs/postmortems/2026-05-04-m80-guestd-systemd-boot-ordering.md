# 2026-05-04 — m80-guestd systemd boot ordering (ubuntu/idle 40% flake)

**Status:** resolved (m80-7tpy; commit `74edca8` lands the inverted-readiness
+ unit-file fix; commit `48f61aa` closes the chain).
**Time to root cause:** ~1 hour, most of it spent on the wrong layer.

## Symptom

The ubuntu image flaked at ~40% on N=20 cold-launch runs. Failures were
non-deterministic: most runs reached guestd readiness within seconds; a
sizable fraction never did, hitting the host's `READY_TIMEOUT` and tearing
down with a vsock-side error. Minimal-image runs were stable.

The visible signature pointed at vsock: the host was polling for `CONNECT`
acks, occasionally seeing `RST` instead, and timing out. Initial framing:
"there's a race in the vsock muxer or in our CONNECT/RST polling logic."

## What it actually was

systemd boot ordering. `m80-guestd.service` had `WantedBy=multi-user.target`.
`multi-user.target` is bimodal: ~1 s on success, ~90 s when
`network-wait-online` times out. On runs where the guest's network DHCP
probe stalled, `multi-user.target` waited the full 90-second budget before
firing, and `m80-guestd.service` started **at T=91 s** — long past the
host's `READY_TIMEOUT`.

There was no race in the vsock muxer. There was no kernel virtio-mmio bug.
The guest simply hadn't started running m80-guestd yet when the host gave up.

## The wrong-direction hour

Roughly an hour was spent on the visible layer and adjacent rabbit holes
before the actual cause surfaced:

- Replaced host CONNECT/RST polling with inverted readiness (guest dials
  out, host `accept()`s on a pre-created `UnixListener`). This was sound
  hardening — and was kept — but addressed a non-cause; the new path also
  flaked.
- Investigated benign `virtio-mmio` kernel warnings on the guest console.
  Unrelated noise.
- Fought intermittent "bash background ghosts" — orphaned background jobs
  in the test scaffolding that occasionally produced phantom failures.
  Distraction; not on the failure path.

Each of these took a serious look because the symptom said "timing/race",
which made every layer that involves timing into a suspect. None of them
needed the hour they got.

## What would have cut the time

m80-guestd's stderr was not piped anywhere the host could read it during
the launch failure. Had it been (`StandardError=journal+console` on the
unit, or direct console output for PID-1 mode), the very first failed run
would have shown a timestamped log line `m80-guestd starting at T=91s` and
the entire investigation would have collapsed in seconds. One line of
visibility would have ended the hour before it began.

## Fix

Three lines in the service unit:

```ini
[Unit]
DefaultDependencies=no
After=local-fs.target workspace.mount

[Install]
WantedBy=basic.target
```

This detaches m80-guestd from `multi-user.target`'s 90-second tail and
starts it as soon as the local filesystem and workspace mount are ready.

The inverted-readiness refactor (`m80-7tpy`) was kept as a hardening
side-effect even though it didn't address the root cause: the new path is
event-driven (`accept()` on a pre-created listener) instead of polling, and
the protocol-version handshake catches version mismatches that the prior
polling path silently fell through. So the wrong-direction hour produced
useful work; it just wasn't the work the symptom demanded.

## Lessons

**Make the other side's stderr visible before forming hypotheses about
timing.** Cross-boundary symptoms (host can't see guest, guest can't see
host) almost always look like timing/race issues at the boundary itself,
because the boundary is what's observable. The actual cause is usually
*beyond* the boundary, in code the visible side can't introspect. Visible
stderr is the cheapest way to make the invisible side speak. It should be
the first move on every cross-boundary investigation, not a debugging tool
of last resort.

**An hour spent on a plausible-sounding layer is not free, even if the work
gets kept.** The inverted-readiness refactor is real value and it ships in
production — but the hour spent on it was not informed by evidence that
that layer was at fault. If the next bug surfaces in a similar shape, the
discipline is the same: visibility first, hypothesis after, even when
"refactor the suspicious layer" looks productive.

**Bimodal dependencies are a distinct hazard from dependencies that just
take a long time.** A target that completes in 1 s most of the time and 90 s
on `network-wait-online` failure is much harder to debug than a target
that always takes 30 s, because the symptom only appears in the slow tail.
Audit unit-file dependencies for bimodal targets specifically; consider
`DefaultDependencies=no` plus explicit narrow `After=` for any service whose
launch budget is tighter than the worst case of its inherited dependencies.

## References

- Bead: `m80-7tpy`
- Commits: `74edca8` (fix), `48f61aa` (CHANGELOG + close)
- The 3-line unit-file change is in the m80-guestd service template;
  search the image-build pipeline for `DefaultDependencies=no`.
- Companion verification doc: `docs/verifications/m80-tv7i.1-vsock-companion-file.md`
