# SmolVM File-Ops Gap

The SmolVM protocol exploration identified direct file verbs as a practical
gap in m80's process-only wire: agent harnesses read, write, stat, list, and
upload files constantly, and doing that through `bash -c` costs a process
spawn, shell quoting, stdout/stderr capture budget, and base64 wrapping.

m80 closes that gap with `m80-6zim`: file-op payloads live in `m80-proto`,
handlers run directly in `m80-guestd`, and `RunningSandbox` exposes wrappers
so host callers do not construct envelopes by hand.
