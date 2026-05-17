# Error Recovery Classification

`m80-firecracker::FcError` remains the concrete public error surface. Callers
that need coarse policy use `FcError::kind()` instead of duplicating a match
over every variant.

The classes are:

- `UserInput`: caller or operator input must change before retry.
- `ResourceExhaustion`: capacity was unavailable now; retrying later may work.
- `Transient`: host, VMM, transport, or timeout failure may work on a fresh
  attempt.
- `Internal`: m80 host mechanics failed; caller request fields are not the
  repair point.
- `GuestOutcome`: guest-facing work reached a terminal outcome or reusable
  handle state.

`FcError::is_retryable()` is true for `ResourceExhaustion` and `Transient`.
`FcError::is_user_error()` is true for `UserInput`.

`FcError` does not expose an anonymous `io::Error` catch-all. I/O failures are
raised through a named variant such as `PathIo`, `HostIo`,
`CommandSpawnFailed`, `KillFailed`, `ReapFailed`, or
`FileUploadReadFailed`, so callers can preserve the operation class while
still reading the source error.

Tests:

- `crates/m80-firecracker/tests/error_variant_displays.rs::admission_refused_is_retryable_resource_exhaustion`
- `crates/m80-firecracker/tests/error_variant_displays.rs::invalid_vm_id_is_user_input`
- `crates/m80-firecracker/tests/error_variant_displays.rs::api_socket_timeout_is_retryable_transient`
- `crates/m80-firecracker/tests/error_variant_displays.rs::idle_timeout_is_guest_outcome_not_retryable`
- `crates/m80-firecracker/tests/error_variant_displays.rs::invalid_state_is_internal_not_retryable`
- `crates/m80-firecracker/tests/error_variant_displays.rs::file_upload_read_failed_displays`
- `crates/m80-firecracker/tests/error_variant_displays.rs::host_io_preserves_operation_label`
