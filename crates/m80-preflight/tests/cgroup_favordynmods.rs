use m80_preflight::HostPrerequisiteCheckId;

#[test]
fn cgroup_favordynmods_check_id_is_stable() {
    assert_eq!(
        HostPrerequisiteCheckId::CgroupFavordynmods.as_str(),
        "cgroup_favordynmods"
    );
    assert_eq!(
        HostPrerequisiteCheckId::CgroupFavordynmods.check_name(),
        "cgroup favordynmods"
    );
}
