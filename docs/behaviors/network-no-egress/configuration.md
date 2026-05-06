# NoEgress Configuration

Behavior capture for `m80-xbn.1`.

## no-egress-reason

m80-built guest-image manifests record a human-readable
`no_egress_reason` value:

```text
no-egress: image is network-neutral; outbound access requires runtime policy
```

This field is operator audit metadata. It preserves the predecessor first-line
no-egress provenance field from `04-infra-and-artifacts.md` and
`DEFAULT_GUESTD_NO_EGRESS_REASON`, but the m80 wording is generic: the image
does not bake an outbound posture into the rootfs. Runtime egress remains a
separate launch policy (`NoEgress` or outbound NAT), and callers must not infer
network access from the image alone.

Regression test:
`crates/m80-image-manifest/tests/no_egress_reason.rs::no_egress_reason_round_trips_for_operator_audit`.
