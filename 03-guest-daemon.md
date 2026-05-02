# 03 — Guest daemon: what runs inside the VM

## Crates examined

- `/tank/projects/predecessor/services/guestd-rs/` — the daemon binary
- `/tank/projects/predecessor/crates/sandbox/agent-guestd-lib/` — guest library
- `/tank/projects/predecessor/crates/sandbox/agent-tool-executor/` — host bridge
- `/tank/projects/predecessor/infra/firecracker/` — image build infra

## What runs inside the VM

`guestd-rs` is a Rust daemon. It listens on **vsock port 9001**, reads
NDJSON `GuestRequest` envelopes from each connection, dispatches one
tool call per connection, returns a `GuestResponse`, flushes filesystems,
closes.

The daemon does **not** run arbitrary commands. It dispatches against a
fixed catalog of six tools (defined in `agent-sandbox-tool-catalog`):

| Tool | What it does |
|---|---|
| `bash` | Run a shell command via `/bin/bash -c` |
| `exec` | Run a program directly with argv (no shell) |
| `read_file` | Read a file in the workspace |
| `write_file` | Write a file to the workspace |
| `build` | `cargo build` in the workspace |
| `test` | `cargo test` in the workspace |

Tool dispatch is gated by the workspace policy that flowed in via the
`GuestRequest`:
- `read_only: true` rejects `bash`, `exec`, `write_file`, `build`, `test`
- `allowed_tools: Some(list)` restricts the surface to that allowlist

(Source: `agent-guestd-lib/src/dispatch.rs:66-85`.)

## Size and structure

| File | LOC | Role |
|---|---|---|
| `services/guestd-rs/src/main.rs` | ~1,479 | Process entry, transport, mount management, handshake |
| `agent-guestd-lib/src/dispatch.rs` | ~126 | `dispatch_tool()` middleware |
| `agent-guestd-lib/src/lib.rs` | ~125 | Re-exports, error mapping |
| `agent-guestd-lib/src/errors.rs` | ~30 | Error types |
| `agent-guestd-lib/tests/` | ~836 | Unit tests |
| **Total** | **~2,596** | |

The binary is a single monolithic file; the library is small. Total guest
surface to extract is modest.

### Main components in `main.rs`

| Lines | Component |
|---|---|
| 76-251 | Config layer (env vars, CLI args, transport selection) |
| 273-330 | Main dispatch loop |
| 273-330 | Accept vsock connection, read envelope, dispatch, serialize, flush |
| 428-439 | Handshake: protocol version + tool spec exchange with host |
| 638-791 | Workspace aliasing: bind-mount guest workspace path to host's logical workspace_root |

The workspace aliasing is generic and worth keeping. It maps a fixed
guest-side mount (`/var/lib/predecessor/workspace`) to whatever
`workspace_root` the host calls it. This means tools see paths matching
the host's view, not the guest's literal mount path.

## Dependencies

`services/guestd-rs/Cargo.toml`:

- `agent-guest-proto` — wire types, handshake
- `agent-sandbox-api` — `ExecutionRequest`, `ExecutionResponse`,
  `SandboxBackend` trait
- `agent-sandbox-tool-catalog` — `local_tool_registry()` (the six tools)
- `agent-sandbox-local` — local execution backend (subprocess, file I/O)
- `agent-tool-registry` — tool validation, schema enforcement
- `tokio` — async runtime
- `tracing`, `serde`, `clap`, etc.

Six predecessor crates pulled in. **The daemon is heavily predecessor-coupled by
construction**: it's specifically built to be the in-VM half of predecessor's
agent-platform tool model.

## What it can do generically

### Workspace mounting (~100 LOC of useful logic)

The aliasing trick — present a stable in-guest path while the host can
think of the workspace at any path — is generic. Keep this.

### Process orchestration

`agent-sandbox-local/src/executor.rs` is the workhorse: spawn a process,
capture stdout/stderr, enforce timeout, collect exit code, package
into a response. About 400 LOC of useful generic logic that the m80 guest
will need.

### NDJSON envelope handling

The frame reader/writer in `agent-guest-proto` is generic NDJSON over
async I/O. ~200 LOC. Keep the pattern, replace the envelope types.

## What's predecessor-specific

### Tool registry validation

`dispatch.rs` calls `local_tool_registry().get(&request.tool_name)` and
rejects unknown tool names. **m80 doesn't have a tool registry.** Drop
this layer; pass `program + args` straight through.

### Workspace policy enforcement

`dispatch.rs:66-85` enforces `read_only` and `allowed_tools` from the
request. **m80's threat model is different**: the user runs the CLI, picks
the image, picks the command — there's no separate "policy authority"
mediating. Drop this enforcement; if the user wants it, they can layer
their own.

### Effect-class semantics

The `EffectClass::ReadOnly | Mutating` field on every request, used to
gate writeback eligibility. Bring this back as a much simpler concept in
m80: writeback is on/off as a CLI flag.

### Artifact capture hints

`captured_artifacts` and `artifact_capture_hints` on the response are
predecessor's mechanism for the host to suggest "also capture this file when
done". Generic enough to keep, but more complexity than v0.1 needs. Drop.

## Image contents (from `prepare-guestd-image.sh`)

The current `guestd-control.ext4` image has:

- Ubuntu LTS base
- `python3` and `nodejs` (no `npm`/`pip`/`pip3`; the script fails if it
  detects them)
- `/usr/local/bin/guestd-rs`
- `/etc/systemd/system/guestd-rs.service` (Type=simple, Restart=on-failure)
- `/etc/systemd/system/var-lib-predecessor-workspace.mount`
  (mounts `/dev/vdb` ext4 at `/var/lib/predecessor/workspace`)
- `/etc/default/guestd-rs` (env file with `LEGACY_FIRECRACKER_NO_EGRESS_REASON`)
- `/var/lib/predecessor/workspace` directory

The image is purpose-built for predecessor's agent stack. **m80 can ship a
simpler image**: any Linux base + an `m80-guestd` binary + a systemd unit +
a workspace mount unit. The python3+nodejs preinstallation is a predecessor
choice; m80 lets users bring their own.

## Cost of the rewrite

| Slice | Estimate |
|---|---|
| Strip tool registry validation, policy enforcement | 0.5d |
| Replace `GuestRequest` with `M80Request` | 0.5d |
| Simplify dispatch to `Command::new(program).args(args)` | 0.5d |
| New systemd units, `/etc/default/m80-guestd`, image build script | 1d |
| Vsock listener (mostly portable) and bind-mount logic (mostly portable) | 1d |
| Tests | 1d |

**Total: 3-4 days** for a working v0.1 guest daemon. Plus ~1 day for
the image-build pipeline rewrite (next file).

## OutboundNat in the guest

For completeness — when `OutboundNat` is enabled, the host writes
configuration into the per-VM rootfs clone *before* boot:

- `/etc/systemd/network/10-predecessor-outbound.network` with static IPv4,
  gateway, MAC
- `/etc/systemd/resolved.conf.d/10-predecessor-dns.conf` with admitted DNS
  resolvers
- DNS resolvers must come from the host's view of `/etc/resolv.conf` or
  `resolvectl dns`, filtered to public-IPv4 only

The guest daemon itself doesn't touch networking — `systemd-networkd` does
the work at boot. m80 inherits this same pattern; the only thing to
rename is "predecessor" → "m80" in the unit names.
