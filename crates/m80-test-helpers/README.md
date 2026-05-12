# m80-test-helpers

Shared test fixtures and utilities for m80 integration tests.

This crate is **dev-dependencies only**. It must never appear in a
`[dependencies]` block, and it is not part of m80's runtime surface. Its only
purpose is to keep repeated integration-test setup code consistent across m80
crates.

## Black-box contract

`m80-test-helpers` provides deterministic, local-only fixtures for tests that
need one of two things:

- serialized environment-variable mutation with automatic restore;
- a minimal HTTP fixture server over a Unix domain socket.

The helpers are intentionally small and concrete. They do not hide real
Firecracker, KVM, jailer, filesystem, or networking behavior behind mocks.

## Public surface

### `env`

- `env_lock() -> &'static Mutex<()>`
  - Returns the process-wide mutex that env-mutating tests must hold.
  - Use it before setting or removing environment variables in tests that can
    run under Rust's parallel test harness.
- `EnvRestore`
  - RAII guard that restores captured environment variables on drop.
- `EnvRestore::capture(keys: &[&'static str]) -> EnvRestore`
  - Captures the current value, or absence, of each key before the test mutates
    process environment.

### `fixture_server`

- `SingleFixtureServer`
  - Runs one HTTP-over-UDS request/response exchange.
  - Public fields:
    - `socket_path: PathBuf` points clients at the bound Unix socket.
- `SingleFixtureServer::spawn(response_bytes: Vec<u8>) -> io::Result<Self>`
  - Binds a temporary Unix socket and serves `response_bytes` to the first
    inbound connection.
- `SingleFixtureServer::join(self) -> FixtureResult`
  - Waits for the server thread and returns the captured request.
- `FixtureResult`
  - Public fields:
    - `request: String` contains the raw HTTP request text.
- `MultiFixtureServer`
  - Runs one HTTP-over-UDS exchange per supplied response.
  - Public fields:
    - `socket_path: PathBuf` points clients at the bound Unix socket.
- `MultiFixtureServer::spawn(responses: Vec<Vec<u8>>) -> io::Result<Self>`
  - Binds a temporary Unix socket and serves each response to one inbound
    connection in order.
- `MultiFixtureServer::join(self) -> Vec<String>`
  - Waits for the server thread and returns all captured requests in order.
- `resp_204() -> Vec<u8>`
  - Builds a minimal `204 No Content` HTTP response.
- `resp_400(body: &str) -> Vec<u8>`
  - Builds a `400 Bad Request` HTTP response with `body` as the JSON payload.

## Dependencies

- `tempfile` for Unix-socket fixture directories.

## Non-goals

- No production dependency surface.
- No real Firecracker, KVM, jailer, cgroup, or network setup.
- No generic HTTP server framework.
- No artifact verification or manifest-schema validation.
