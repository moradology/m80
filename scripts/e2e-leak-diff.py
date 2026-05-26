#!/usr/bin/env python3
"""Diff two e2e-reap dry-run reports for new m80-owned residue."""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
from typing import Any


SCHEMA_VERSION = 1


def _resource_key(source: str, item: dict[str, Any]) -> tuple[str, str, str, str]:
    return (
        source,
        str(item.get("kind", "")),
        str(item.get("target", "")),
        str(item.get("reason", "")) if source == "skipped" else "",
    )


def _resource_items(report: dict[str, Any]) -> list[dict[str, Any]]:
    items = []
    for source in ("actions", "skipped"):
        for raw in report.get(source, []):
            if isinstance(raw, dict):
                item = dict(raw)
                item["source"] = source
                items.append(item)
    return items


def new_resources(before: dict[str, Any], after: dict[str, Any]) -> list[dict[str, Any]]:
    """Return reaper-reported resources present after but absent before."""
    before_keys = {
        _resource_key(str(item["source"]), item)
        for item in _resource_items(before)
    }
    return [
        item
        for item in _resource_items(after)
        if _resource_key(str(item["source"]), item) not in before_keys
    ]


def diff_report(before: dict[str, Any], after: dict[str, Any]) -> dict[str, Any]:
    resources = new_resources(before, after)
    return {
        "schema_version": SCHEMA_VERSION,
        "before_resource_count": len(_resource_items(before)),
        "after_resource_count": len(_resource_items(after)),
        "new_resources": resources,
    }


def _read_json(path: pathlib.Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError(f"{path}: report must be a JSON object")
    return data


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=pathlib.Path)
    parser.add_argument("after", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    args = parser.parse_args(argv)

    try:
        report = diff_report(_read_json(args.before), _read_json(args.after))
    except ValueError as exc:
        print(exc, file=sys.stderr)
        return 2
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 1 if report["new_resources"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
