# Logging

m80 writes VM-mechanics diagnostics under each VM run directory. These files are
operator artifacts, not an API for untrusted workloads.

## console.log

`console.log` contains Firecracker stdout/stderr and guest serial-console
output. The guest can influence this file by writing to its console, so treat
the file as guest-controlled text:

- do not ship `console.log` to third-party log destinations unless that
  destination is approved for guest data;
- redact or exclude `console.log` in broad log collectors by default;
- do not parse it as trusted host events;
- expect arbitrary bytes, ANSI control sequences, and misleading line prefixes.

m80 caps persisted `console.log` bytes at 2 MiB per VM. The host still drains
stdout/stderr after the cap so Firecracker is not blocked by a full pipe, but
additional guest-controlled console output is discarded.

Host lifecycle and phase diagnostics that m80 emits itself live in
`diagnostics.jsonl` and stderr phase events. Guest boot milestone lines parsed
from `console.log` are allowlisted before they become host phase names:
`name=` may contain only lowercase ASCII letters, digits, and underscores.

