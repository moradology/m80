# Host Setup Operations

`docs/ops/host-setup.md` is the operator-facing production checklist for hosts
that build, deploy, or run m80. It defines the three-identity model:

- build identity may hold Docker daemon access and builds artifacts;
- deploy identity installs signed binaries and artifacts;
- run identity starts m80 and must not hold Docker socket access.

The document treats Docker socket access as host-root-equivalent, requires
artifact and run-root ownership/mode hygiene, documents Cargo source override
risk, recommends vendored/offline release builds, records runtime host
preconditions, and states that CI publishing jobs are deploy authority.

Test: `crates/m80-preflight/tests/ops_host_setup_docs.rs::host_setup_doc_pins_security_sections`.
