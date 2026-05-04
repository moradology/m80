//! Real-host integration tests; `#[ignore]`d so CI without cgroup v2 skips.
//! Run with: `sudo cargo test -p m80-cgroup -- --ignored`.

#[test]
#[ignore]
fn probe_returns_ok_on_unified_v2_host() {
    m80_cgroup::Subtree::probe().expect("probe() must return Ok on a unified-v2 host");
}
