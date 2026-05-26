const MATERIALIZED: &str = include_str!("../../src/materialized.rs");
const PLAN: &str = include_str!("../../src/plan.rs");

#[test]
fn materialized_jail_drop_warnings_include_vm_id() {
    assert_source_contains(
        MATERIALIZED,
        &[
            "pub(crate) vm_id: String,",
            "pub(crate) fn vm_id_for_plan(plan: &Plan) -> String",
            ".arg(&self.vm_id)",
            "vm_id = %self.vm_id",
            "drop: bind umount failed",
            "drop: placeholder unlink failed",
            "drop: directory cleanup failed",
        ],
    );
    assert_source_contains(
        PLAN,
        &[
            "let vm_id = crate::materialized::vm_id_for_plan(&self);",
            "vm_id,",
        ],
    );
}

fn assert_source_contains(source: &str, needles: &[&str]) {
    for needle in needles {
        assert!(source.contains(needle), "missing source needle: {needle}");
    }
}
