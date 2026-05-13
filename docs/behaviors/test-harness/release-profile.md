# Release Profile Measurement

Bead: `m80-jp6ik.18`.

The release-profile measurement compares the default Cargo release build for
`m80-cli` against the tuned root release profile:

- `lto = "thin"`
- `codegen-units = 1`
- `strip = "symbols"`
- `panic = "abort"`

The evidence artifact is
`crates/m80-firecracker/benches/snapshots/release-profile-delta.json`, with the
human interpretation in `docs/perf/release-profile.md`.

The test profile is not overridden in `Cargo.toml`. Cargo ignores explicit
`panic` settings for the `test` profile, and `cargo rustc -p m80-cli --test
parse_args -- --print cfg` reports `panic="unwind"`. That keeps test diagnostics
unwound while release binaries abort on panic.
