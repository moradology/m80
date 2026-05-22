#!/usr/bin/env python3
"""Render the public quickstart troubleshooting matrix from JSON."""

from __future__ import annotations

import argparse
import html
import json
from pathlib import Path
import sys
from typing import Any


DEFAULT_MATRIX = Path("docs/behaviors/release/quickstart-troubleshooting-matrix.json")
DEFAULT_DOC = Path("docs/behaviors/release/quickstart-troubleshooting-matrix.md")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", type=Path, default=DEFAULT_MATRIX)
    parser.add_argument("--doc", type=Path, default=DEFAULT_DOC)
    parser.add_argument("--check", action="store_true", help="fail when the committed doc is stale")
    parser.add_argument("--write", action="store_true", help="write the rendered doc instead of printing it")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    rendered = render_doc(read_json(args.matrix), matrix_path=args.matrix)
    if args.check:
        try:
            existing = args.doc.read_text()
        except FileNotFoundError:
            print(f"quickstart troubleshooting doc missing: {args.doc}", file=sys.stderr)
            return 1
        if existing != rendered:
            print(f"quickstart troubleshooting doc is stale: {args.doc}", file=sys.stderr)
            print(f"repair: python3 {Path(__file__).as_posix()} --write", file=sys.stderr)
            return 1
        print(f"quickstart troubleshooting doc current: {args.doc}")
        return 0
    if args.write:
        args.doc.write_text(rendered)
        print(f"rendered quickstart troubleshooting doc: {args.doc}")
        return 0
    print(rendered, end="")
    return 0


def read_json(path: Path) -> dict[str, Any]:
    try:
        with path.open() as f:
            value = json.load(f)
    except FileNotFoundError as exc:
        raise SystemExit(f"quickstart troubleshooting matrix missing: {path}") from exc
    except json.JSONDecodeError as exc:
        raise SystemExit(f"{path}: invalid JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise SystemExit(f"{path}: matrix must be a JSON object")
    return value


def render_doc(matrix: dict[str, Any], *, matrix_path: Path) -> str:
    rows = matrix.get("rows", [])
    lines = [
        "# Quickstart Troubleshooting Matrix",
        "",
        "The public quickstart uses stable failure IDs so support reports, docs, and",
        "fixtures talk about the same first-run problem. The IDs below are stable enough",
        "for bug reports. Rename one only with a replacement row and release-note",
        "migration.",
        "",
        "Generated from",
        f"[`{matrix_path.name}`]({matrix_path.name}) by",
        "`scripts/render-quickstart-troubleshooting.py`; do not edit this table by hand.",
        "",
        "| ID | Symptom | Likely failing command | Diagnostic command | Next command | Action | Owner | Source coverage |",
        "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for row in rows:
        lines.append(render_row(row))
    lines.append("")
    return "\n".join(lines)


def render_row(row: dict[str, Any]) -> str:
    row_id = require_str(row, "id")
    cells = [
        f'<a id="{html.escape(row_id)}"></a>`{escape_cell(row_id)}`',
        escape_cell(require_str(row, "title")),
        command_cell(require_str(row, "likely_failing_command")),
        command_cell(require_str(row, "diagnostic_command")),
        command_cell(require_str(row, "repair_command")),
        f"`{escape_cell(require_str(row, 'action_kind'))}`",
        f"`{escape_cell(require_str(row, 'owner_action'))}`",
        source_cell(row.get("source_mappings")),
    ]
    return "| " + " | ".join(cells) + " |"


def command_cell(command: str) -> str:
    return f"<code>{escape_cell(command)}</code>"


def source_cell(value: object) -> str:
    if not isinstance(value, list):
        return ""
    refs: list[str] = []
    for item in value:
        if not isinstance(item, dict):
            continue
        kind = item.get("kind")
        ref = item.get("ref")
        if isinstance(kind, str) and isinstance(ref, str):
            refs.append(f"<code>{escape_cell(kind)}:{ref}</code>")
    return "<br>".join(refs)


def escape_cell(value: str) -> str:
    return html.escape(value, quote=False).replace("|", "&#124;")


def require_str(row: dict[str, Any], field: str) -> str:
    value = row.get(field)
    if not isinstance(value, str):
        raise SystemExit(f"row {row.get('id', '<unknown>')}: {field} must be a string")
    return value


if __name__ == "__main__":
    raise SystemExit(main())
