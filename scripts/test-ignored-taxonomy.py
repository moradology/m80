#!/usr/bin/env python3
"""Verify every Rust #[ignore] has a machine-readable reason token."""

from __future__ import annotations

import pathlib
import re
import sys


ALLOWED = {
    "requires-kvm",
    "requires-root",
    "requires-network-namespace",
    "slow",
    "requires-cgroup-v2",
    "requires-artifacts",
    "requires-external-network",
    "requires-malicious-artifacts",
    "requires-docker",
    "requires-mount-namespace",
    "requires-loop-device",
    "requires-snapshot-support",
    "requires-debugfs",
    "requires-erofs-tool",
    "requires-pmem",
    "measurement",
    "manual",
}

IGNORE_ATTR = re.compile(
    r"(?m)^(?P<indent>[ \t]*)#\s*\[\s*ignore(?:\s*=\s*\"(?P<reason>[^\"]*)\")?\s*\]"
)


def main() -> int:
    failures: list[str] = []
    root = pathlib.Path(__file__).resolve().parents[1]
    for path in sorted((root / "crates").rglob("*.rs")):
        text = path.read_text()
        for match in IGNORE_ATTR.finditer(text):
            line = text.count("\n", 0, match.start()) + 1
            reason = match.group("reason")
            if reason is None:
                failures.append(f"{path.relative_to(root)}:{line}: bare #[ignore]")
                continue
            tokens = reason.split()
            unknown = [token for token in tokens if token not in ALLOWED]
            if not tokens:
                failures.append(f"{path.relative_to(root)}:{line}: empty #[ignore] reason")
            elif unknown:
                failures.append(
                    f"{path.relative_to(root)}:{line}: unknown ignore token(s): {', '.join(unknown)}"
                )

    if failures:
        print("ignored-test taxonomy violations:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("ignored-test taxonomy ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
