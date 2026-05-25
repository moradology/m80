#!/usr/bin/env python3
"""Behavior-identical helpers shared by the release/CI Python scripts.

Only helpers whose every call site shared *identical* behavior are extracted
here. Deliberately-divergent variants (read_json, write_json, check,
require_nonempty_*, sha256_ref) stay local to their scripts because unifying
them would change exception type, operator-facing message, or side effects.
"""

from __future__ import annotations

from datetime import datetime
import hashlib
from pathlib import Path


def require(condition: object, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def parse_timestamp(value: object) -> datetime | None:
    if not isinstance(value, str) or not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
