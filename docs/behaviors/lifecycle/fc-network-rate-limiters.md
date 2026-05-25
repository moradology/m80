# Firecracker Network Rate Limiters

`m80-firecracker-client` models Firecracker `v1.15.1`
`NetworkInterface` rate limiter fields directly:

- `rx_rate_limiter`
- `tx_rate_limiter`

Each field is optional and serializes only when the caller supplies a
`RateLimiterConfig`. A `RateLimiterConfig` may contain a `bandwidth`
`TokenBucketConfig`, an `ops` `TokenBucketConfig`, or both.

`NoEgress` launches emit no network-interface `PUT`. For the network branches
that do emit one (`JoinNetns` and `OutboundNat`), m80's launch planner
currently sets both rate limiters to `None`, so those launches keep their
previous Firecracker request body unless a future caller deliberately opts into
rate limiting.

The earlier `m80-2ggw.3.6` queue-size wording was stale. Firecracker `v1.15.1`
does not expose `rx_queue_size` or `tx_queue_size` REST fields on
`NetworkInterface`. m80 must not send those names because Firecracker rejects
unknown request fields.

## Validation

- `crates/m80-firecracker-client/tests/network_interface_config_round_trip.rs::put_network_interface_sends_rate_limiters_when_set`
  pins the RX/TX rate limiter JSON shape.
- `crates/m80-firecracker-client/tests/network_interface_config_round_trip.rs::put_network_interface_omits_absent_guest_mac`
  pins omission of absent rate limiter fields.
- `crates/m80-firecracker/src/preboot_tests.rs::outbound_nat_network_interface_put_after_drives_and_before_vsock`
  pins m80's current launch planner default of no network-interface rate
  limiters.
