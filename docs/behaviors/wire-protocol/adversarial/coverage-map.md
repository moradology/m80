# Adversarial Guestd Wire Coverage Map

Behaviors captured by `m80-g0v8.12.9`.

## Scope

This map tracks guest-to-host wire attacks where the guest peer is malicious or
broken. The L12 real-KVM path is the authority for proving that the host sees
the same failure through Firecracker, vsock, request handling, diagnostics, and
cleanup that the lower-level protocol tests already model.

Existing L2 and unit tests remain valuable, but they are not substitutes for
L12. They prove the typed protocol surface in isolation. L12 proves that the
same attack survives the full VM boundary and fails in the user-visible shape
we intend.

## Coverage Matrix

| Attack class | L12 bead | Real-KVM malicious guestd path | Existing synthetic / L2 path | Expected host result | Diagnostics and cleanup | Residual gap |
|---|---|---|---|---|---|---|
| Oversized frame length | `m80-g0v8.12.2` | `crates/m80-guestd-malicious` mode `oversized_length` emits a prefix larger than `MAX_FRAME_BYTES` without a body; `crates/m80-firecracker/tests/malicious/oversized_length.rs` runs it through the real-KVM path. | `crates/m80-proto/tests/framing_size_cap.rs::rejects_frame_above_4mib_with_oversized_error`; `crates/m80-guestd/tests/handle_connection.rs::oversized_initial_frame_drops_connection_without_response`; `crates/m80-firecracker/tests/wire_frame_boundaries_real_kvm.rs::frame_length_over_max_drops_current_channel_only` covers the host-to-guest/control direction. | `WireProtocolError::OversizedFrame` via `ProtoError::OversizedPayload`, before allocating the announced body. | Current channel closes, diagnostic names oversized payload size and limit, RSS growth is bounded, sandbox cleanup completes. | Closed for the guest-to-host oversized length-prefix path; residual flood/slowloris resource attacks remain under `m80-g0v8.12.8`. |
| Truncated frame body | `m80-g0v8.12.3` | `crates/m80-guestd-malicious` mode `truncated_frame` writes a valid prefix, writes fewer body bytes than promised, then closes; `crates/m80-firecracker/tests/malicious/truncated_frame.rs` runs it through the real-KVM path. | `crates/m80-proto/tests/framing_parse_failure.rs::short_frame_body_returns_unexpected_eof`; lifecycle protocol mapping covers EOF as `DisconnectBeforeTerminal`. | Typed under-read / disconnect error, currently `WireProtocolError::DisconnectBeforeTerminal` when surfaced through lifecycle receive. | Reader unblocks within the test timeout, request activity guard is released, diagnostics record the disconnect, cleanup can tear down the sandbox. | Closed for the guest-to-host closed-short-body path; slowloris no-progress timing remains under `m80-g0v8.12.8`. |
| Malformed protobuf body | Covered adjacent to L12; no separate L12 leaf | Existing real-KVM boundary test poisons a stream with malformed bytes from the host side; a malicious guestd variant would mirror that direction. | `crates/m80-proto/tests/framing_parse_failure.rs::malformed_protobuf_returns_error_and_drops_connection`; `crates/m80-firecracker/tests/wire_frame_boundaries_real_kvm.rs::malformed_frame_mid_stream_maps_to_user_visible_error`. | `ProtoError::MalformedPayload` maps to `WireProtocolError::MalformedPayload` where the host is decoding guest output. | Channel teardown, guest console `protocol_error` marker for host-control poisoning, fresh channel survival for the current real-KVM boundary case. | No dedicated L12 malicious guestd leaf; add one only if L12.4 unknown-variant coverage does not also exercise malformed decode diagnostics. |
| Unknown envelope variant | `m80-g0v8.12.4` | `crates/m80-guestd-malicious` mode `unknown_variant` bypasses safe encoding and emits a valid envelope with out-of-schema payload field 255; `crates/m80-firecracker/tests/malicious/unknown_variant.rs` runs it through the real-KVM path. | `crates/m80-proto/tests/envelope_unknown_fields.rs` rejects unknown top-level envelope fields before prost can drop them. | `WireProtocolError::MalformedPeer` naming the offending envelope field. | Channel closes without panic, diagnostics include observed unknown field, sandbox cleanup completes. | Closed for the guest-to-host unknown payload field path. |
| Response type mismatch | `m80-g0v8.12.5` | `crates/m80-guestd-malicious` mode `response_type_mismatch` reads a request, echoes its `request_id`, and replies with an `exec_exit` envelope carrying a `file_read_response` payload; `crates/m80-firecracker/tests/malicious/response_type_mismatch.rs` runs it through the real-KVM path. | `crates/m80-guestd-malicious` unit coverage pins the exact mismatched frame; file upload ack tests cover wrong upload/sequence shape. | `WireProtocolError::MalformedPeer` naming expected `exec_exit` and observed `file_read_response`. | Affected channel closes, diagnostics include expected/observed payload kinds, and sandbox cleanup completes. | Closed for the guest-to-host typed response mismatch path. |
| Wrong or stale `request_id` | `m80-g0v8.12.6` | `crates/m80-guestd-malicious` mode `bogus_request_id` reads a request and replies with a valid `exec_exit` frame for `malicious-stale-request-id`; `crates/m80-firecracker/tests/malicious/bogus_request_id.rs` runs it through the real-KVM path. | `crates/m80-firecracker/src/lifecycle/exec.rs::tests::response_frame_rejects_stale_request_id`, `response_frame_rejects_missing_request_id`, and `cancel_ack_rejects_stale_request_id`; `crates/m80-guestd/tests/handle_connection.rs::cancel_wrong_request_id_returns_already_exited` covers guest-side cancel semantics. | `WireProtocolError::RequestIdMismatch` with expected and observed request ids. | Affected request fails fast, diagnostics include expected/observed ids, no cross-request data leak, and sandbox cleanup completes. | Closed for the guest-to-host bogus request-id response path. |
| Unsolicited response before request | `m80-g0v8.12.7` | Planned `unsolicited_response` malicious guestd writes a response at boot before reading any host request. | Synthetic coverage is partial: host lifecycle assumes one connection carries one active request, so unsolicited output is usually modeled as an unexpected frame once a caller is reading. | Policy must be pinned by `m80-g0v8.12.7`: either drop-and-continue or typed unsolicited-response teardown. | Diagnostic names unsolicited response and current policy; if drop-and-continue, later request proves usability; if teardown, cleanup proves no leak. | Host policy is intentionally not declared complete until `m80-g0v8.12.7` chooses and pins it. |
| Unsolicited flood | `m80-g0v8.12.8` | Planned `unsolicited_flood` malicious guestd sends many unsolicited messages back-to-back. | Bounded-memory stream tests cover normal producer queues, not adversarial unsolicited floods. | Documented rate-limit, timeout, backpressure, or teardown policy; no unbounded host CPU or memory growth. | `/proc/self/status` or equivalent snapshots bound host resource growth; diagnostics name policy trigger; sandbox cleanup completes. | Real malicious guest-to-host flood remains open until `m80-g0v8.12.8` closes. |
| Slowloris partial frame | `m80-g0v8.12.8` | Planned `slowloris` malicious guestd opens the channel, writes a prefix or partial body, then stalls. | `framing_parse_failure` covers closed short body, but not indefinite no-progress timing. | Typed timeout or policy error within the documented bound. | Reader thread joins or request activity guard releases after timeout, diagnostics record no-progress timeout, sandbox cleanup completes. | Real no-progress timing path remains open until `m80-g0v8.12.8` closes. |

## Expected Error And Diagnostic Policy

- Oversized length is an allocation guard: reject from the length prefix with
  `ProtoError::OversizedPayload`, surface `WireProtocolError::OversizedFrame`,
  and include the announced size plus configured limit.
- Truncated or disconnected frames surface as an under-read. Through lifecycle
  request handling, the user-visible error is `DisconnectBeforeTerminal`
  unless a narrower truncated-frame variant is added in the same hard cutover.
- Malformed protobuf and unknown generated-wire values are schema failures.
  They must fail the current channel, not panic or silently skip bytes.
- Response type mismatch and request-id mismatch are request-correlation
  failures. The diagnostic must name the expected value and the observed value
  whenever the host has both.
- Unsolicited messages, floods, and slowloris behavior are policy surfaces, not
  parser quirks. The child bead that pins each policy owns the final error name,
  timeout bound, resource bound, and channel-survival rule.
- Every real-KVM L12 test must prove cleanup, not just error classification:
  the reader must unblock, any request permit or activity guard must release,
  diagnostics must be present in the run directory or guest console, and the VM
  must tear down without run-root leakage.

## Residual Gaps

Bogus workload result claims remain out of scope for the current wire harness.
If guestd sends `ExecExit { exit_code: 0 }` and plausible stdout bytes, the
host has no independent runtime proof that the guest workload actually exited
that way. Closing that gap would require a separate runtime cross-check or
attestation mechanism, not just a stronger protobuf parser.

Wall-clock-coordinated attacks are only partly covered by `m80-g0v8.12.8`.
The flood and slowloris modes pin bounded no-progress and resource behavior,
but they do not exhaustively prove every scheduler race a malicious peer could
attempt against the host reader. Any future timing race coverage must use
deterministic harness controls rather than relying on wall-clock luck.

Host-to-guest/control-path poisoning already has real-KVM coverage in
`wire_frame_boundaries_real_kvm.rs`, including malformed mid-stream handling
and fresh-channel survival. That evidence is adjacent support, not completion
evidence for the L12 guest-to-host malicious guestd leaves.

## Coverage Ownership

`m80-g0v8.12.1` owns the malicious guestd test artifact. `m80-g0v8.12.2`
through `m80-g0v8.12.8` own the individual real-KVM attack tests and their
per-class behavior docs. This map is complete when it accurately names those
paths, separates planned L12 coverage from existing synthetic coverage, and
keeps the residual gaps explicit.
