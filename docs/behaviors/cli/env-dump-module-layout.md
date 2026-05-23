# Env Dump Module Layout

`m80 env` keeps command dispatch and data collection in
`crates/m80-cli/src/cmds/env.rs`, with focused private modules for the pieces
that otherwise make the diagnostic surface hard to review:

- `model.rs` owns the serializable JSON shape.
- `render.rs` owns human output.
- `tests.rs` owns the command-level env dump regression tests.

The split does not change the command surface, JSON fields, profile selection,
host checks, preflight collection, or human output. It keeps
`crates/m80-cli/src/cmds/env.rs` below the preferred review threshold while
leaving the existing env dump behavior pinned by the same tests.

Verification:

- `crates/m80-cli/src/cmds/env/tests.rs::env_json_has_data_version_and_host_sections`
- `crates/m80-cli/src/cmds/env/tests.rs::human_output_names_bug_report_fields`
- `crates/m80-cli/src/cmds/env/tests.rs::env_json_reports_selected_installed_profile_paths`
- `crates/m80-cli/src/profile/report.rs` tests for active pointer and missing
  profile-path diagnostics.
