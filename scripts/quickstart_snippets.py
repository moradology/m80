"""Shared quickstart snippet contract for public docs and release proof CI."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import re
import shlex

from release_url_contract import (
    latest_install_command,
    pinned_install_command,
    verified_install_handoff_block,
)


SNIPPET_MARKER_RE = re.compile(
    r"^<!--\s*m80:quickstart-snippet\s+([a-z0-9-]+)\s+(start|end)\s*-->$"
)


@dataclass(frozen=True)
class QuickstartSnippet:
    name: str
    body: str


def quickstart_smoke_command() -> str:
    return "m80 run -- echo hello"


def quickstart_smoke_argv() -> list[str]:
    return shlex.split(quickstart_smoke_command())


def expected_quickstart_snippets(release_tag: str = "<version>") -> dict[str, str]:
    return {
        "post-install-smoke": quickstart_smoke_command(),
        "latest-install": latest_install_command(),
        "pinned-install": pinned_install_command(release_tag),
        "verified-install-handoff": verified_install_handoff_block(release_tag),
    }


def install_snippets(release_tag: str = "<version>") -> list[QuickstartSnippet]:
    snippets = expected_quickstart_snippets(release_tag)
    return [
        QuickstartSnippet("latest-install", snippets["latest-install"]),
        QuickstartSnippet("pinned-install", snippets["pinned-install"]),
        QuickstartSnippet("verified-install-handoff", snippets["verified-install-handoff"]),
    ]


def extract_marked_quickstart_snippets(path: Path) -> dict[str, str]:
    active_name: str | None = None
    active_lines: list[str] = []
    snippets: dict[str, str] = {}

    for line_number, line in enumerate(path.read_text().splitlines(), start=1):
        marker = SNIPPET_MARKER_RE.fullmatch(line.strip())
        if marker is None:
            if active_name is not None:
                active_lines.append(line)
            continue

        name, kind = marker.groups()
        if kind == "start":
            if active_name is not None:
                raise ValueError(f"{path}:{line_number}: nested quickstart snippet {name!r}")
            if name in snippets:
                raise ValueError(f"{path}:{line_number}: duplicate quickstart snippet {name!r}")
            active_name = name
            active_lines = []
            continue

        if active_name != name:
            raise ValueError(f"{path}:{line_number}: unmatched quickstart snippet end {name!r}")
        snippets[name] = normalize_snippet_body(active_lines)
        active_name = None
        active_lines = []

    if active_name is not None:
        raise ValueError(f"{path}: missing end marker for quickstart snippet {active_name!r}")
    return snippets


def normalize_snippet_body(lines: list[str]) -> str:
    body = trim_blank_edges(lines)
    if len(body) >= 2 and body[0].startswith("```") and body[-1] == "```":
        body = trim_blank_edges(body[1:-1])
    return "\n".join(body)


def trim_blank_edges(lines: list[str]) -> list[str]:
    start = 0
    end = len(lines)
    while start < end and not lines[start].strip():
        start += 1
    while end > start and not lines[end - 1].strip():
        end -= 1
    return lines[start:end]
