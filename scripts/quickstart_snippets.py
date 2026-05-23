"""Shared quickstart snippet contract for public docs and release proof CI."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import re
import shlex

from release_url_contract import (
    latest_install_command,
    pinned_install_command,
    release_repository,
    verified_install_handoff_block,
)


SNIPPET_MARKER_RE = re.compile(
    r"^<!--\s*m80:quickstart-snippet\s+([a-z0-9-]+)\s+(start|end)\s*-->$"
)
INSTALL_URL_RE = re.compile(
    r"https://github\.com/([^/\s]+/[^/\s]+)/(?:releases/latest/download|releases/download/[^/\s]+)/install\.sh"
)
CONCRETE_PINNED_INSTALL_RE = re.compile(
    r"^curl -fsSL https://github\.com/[^/\s]+/[^/\s]+/releases/download/v[0-9]+\.[0-9]+\.[0-9]+/install\.sh \| sudo sh$"
)
ARTIFACT_ONLY_LATEST_RE = re.compile(
    r"https://github\.com/[^/\s]+/[^/\s]+/releases/latest/download/[^`'\"\s]+\.tar\.gz"
)
RAW_MAIN_INSTALL_RE = re.compile(
    r"https://raw\.githubusercontent\.com/[^/\s]+/[^/\s]+/main/[^`'\"\s]*install\.sh"
)
DEPRECATED_QUICKSTART_MARKER_RE = re.compile(
    r"<!--\s*m80:deprecated-quickstart\s+start\s*-->.*?<!--\s*m80:deprecated-quickstart\s+end\s*-->",
    re.DOTALL,
)
URL_RE = re.compile(r"https?://[^`'\"\s<>]+")
DEPRECATED_QUICKSTART_ALLOWED_URLS = {
    "https://github.com/moradology/m80/releases/latest/download/m80-linux-x86_64-minimal-artifacts.tar.gz",
    "https://raw.githubusercontent.com/moradology/m80/main/scripts/install.sh",
}
PUBLIC_COMMAND_DOCS = (
    "README.md",
    "crates/*/README.md",
    "docs/runbook/**/*.md",
    "docs/behaviors/**/*.md",
)
LEGACY_INTERNAL_DOCS = {
    "docs/behaviors/release/legacy-quickstart-hard-cutover.md",
    "docs/behaviors/release/docs-quickstart-gate.md",
    "docs/behaviors/cli/egress-policy.md",
    "docs/behaviors/cli/installed-default-profile.md",
    "docs/behaviors/cli/product-surface.md",
}
TROUBLESHOOTING_CONTEXT_WORDS = ("repair", "troubleshoot", "troubleshooting", "diagnostic", "rollback")
FRESHNESS_STATUS_MARKER_RE = re.compile(r"m80:freshness-status\s+start")
FRESHNESS_STATUS_GUARD_WORDS = ("public installer status", "proof")
FRESHNESS_STATUS_VALUES = ("pending", "scaffolded", "public proof green", "stale", "failed")


@dataclass(frozen=True)
class QuickstartSnippet:
    name: str
    body: str


@dataclass(frozen=True)
class PublicCommandSnippet:
    path: Path
    line: int
    body: str
    classification: str


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
    return extract_marked_quickstart_snippets_from_text(path.read_text(), source=str(path))


def extract_marked_quickstart_snippets_from_text(text: str, *, source: str) -> dict[str, str]:
    active_name: str | None = None
    active_lines: list[str] = []
    snippets: dict[str, str] = {}

    for line_number, line in enumerate(text.splitlines(), start=1):
        marker = SNIPPET_MARKER_RE.fullmatch(line.strip())
        if marker is None:
            if active_name is not None:
                active_lines.append(line)
            continue

        name, kind = marker.groups()
        if kind == "start":
            if active_name is not None:
                raise ValueError(f"{source}:{line_number}: nested quickstart snippet {name!r}")
            if name in snippets:
                raise ValueError(f"{source}:{line_number}: duplicate quickstart snippet {name!r}")
            active_name = name
            active_lines = []
            continue

        if active_name != name:
            raise ValueError(f"{source}:{line_number}: unmatched quickstart snippet end {name!r}")
        snippets[name] = normalize_snippet_body(active_lines)
        active_name = None
        active_lines = []

    if active_name is not None:
        raise ValueError(f"{source}: missing end marker for quickstart snippet {active_name!r}")
    return snippets


def normalize_snippet_body(lines: list[str]) -> str:
    body = trim_blank_edges(lines)
    if len(body) >= 2 and body[0].startswith("```") and body[-1] == "```":
        body = trim_blank_edges(body[1:-1])
    body = dedent_common_indent(body)
    return "\n".join(body)


def trim_blank_edges(lines: list[str]) -> list[str]:
    start = 0
    end = len(lines)
    while start < end and not lines[start].strip():
        start += 1
    while end > start and not lines[end - 1].strip():
        end -= 1
    return lines[start:end]


def dedent_common_indent(lines: list[str]) -> list[str]:
    indents = [len(line) - len(line.lstrip(" ")) for line in lines if line.strip()]
    if not indents:
        return lines
    prefix = min(indents)
    if prefix == 0:
        return lines
    return [line[prefix:] if line.strip() else line for line in lines]


def public_command_inventory(root: Path) -> list[PublicCommandSnippet]:
    snippets: list[PublicCommandSnippet] = []
    for path in public_command_doc_paths(root):
        validate_public_command_urls(path.read_text(), path.relative_to(root))
        snippets.extend(extract_public_command_snippets(path, root=root))
    return snippets


def public_command_doc_paths(root: Path) -> list[Path]:
    paths: set[Path] = set()
    for pattern in PUBLIC_COMMAND_DOCS:
        paths.update(path for path in root.glob(pattern) if path.is_file())
    return sorted(paths)


def extract_public_command_snippets(path: Path, *, root: Path) -> list[PublicCommandSnippet]:
    lines = path.read_text().splitlines()
    snippets: list[PublicCommandSnippet] = []
    in_fence = False
    fence_start = 0
    block: list[str] = []
    for line_number, line in enumerate(lines, start=1):
        if line.lstrip().startswith("```"):
            if in_fence:
                body = normalize_snippet_body(block)
                if is_public_command_block(body):
                    context = surrounding_context(lines, fence_start, line_number)
                    relative = path.relative_to(root)
                    classification = classify_public_command_snippet(body, relative, context)
                    snippets.append(
                        PublicCommandSnippet(
                            path=relative,
                            line=fence_start,
                            body=body,
                            classification=classification,
                        )
                    )
                in_fence = False
                fence_start = 0
                block = []
            else:
                in_fence = True
                fence_start = line_number
                block = []
            continue
        if in_fence:
            block.append(line)
    return snippets


def is_public_command_block(body: str) -> bool:
    lines = [line.strip() for line in body.splitlines() if line.strip()]
    if not lines:
        return False
    for line in lines:
        if line.startswith("curl ") and ("install.sh" in line or "releases/" in line):
            return True
        if line.startswith("sudo sh ") and "install.sh" in line:
            return True
        if line.startswith("sh ") and "install.sh" in line:
            return True
        if line.startswith("m80 install") or line.startswith("m80 quickstart"):
            return True
        if line == quickstart_smoke_command():
            return True
    return False


def classify_public_command_snippet(body: str, relative_path: Path, context: str) -> str:
    validate_public_command_urls(body, relative_path)
    expected = expected_quickstart_snippets()
    if body == expected["post-install-smoke"]:
        return "common"
    if body == expected["latest-install"]:
        if not has_freshness_status_guard(context):
            raise ValueError(
                f"{relative_path}: latest install snippet must be guarded by a generated freshness status note"
            )
        return "common"
    if body == expected["pinned-install"]:
        return "pinned"
    if body == expected["verified-install-handoff"]:
        return "verified/operator"
    if is_legacy_internal_reference(body, relative_path, expected):
        return "legacy-internal"
    if is_install_status_command(body):
        return "troubleshooting"
    if is_rollback_cleanup_command(body, context):
        return "troubleshooting"
    if is_troubleshooting_pinned_install(body, context):
        return "troubleshooting"
    raise ValueError(
        f"{relative_path}: unclassified public command snippet starting with {first_nonempty_line(body)!r}"
    )


def validate_public_command_urls(body: str, relative_path: Path) -> None:
    scanned_body = body
    if str(relative_path) == "docs/behaviors/release/legacy-quickstart-hard-cutover.md":
        scanned_body = strip_deprecated_quickstart_marker_blocks(scanned_body, relative_path)
    if RAW_MAIN_INSTALL_RE.search(scanned_body) is not None:
        raise ValueError(f"{relative_path}: public install snippets must not use mutable raw main URLs")
    match = ARTIFACT_ONLY_LATEST_RE.search(scanned_body)
    if match is not None:
        raise ValueError(f"{relative_path}: artifact-only latest URL is not a public quickstart: {match.group(0)}")
    for match in INSTALL_URL_RE.finditer(scanned_body):
        repository = match.group(1)
        if repository != release_repository():
            raise ValueError(
                f"{relative_path}: install URL uses {repository}, expected {release_repository()}"
            )


def strip_deprecated_quickstart_marker_blocks(body: str, relative_path: Path) -> str:
    def checked_replacement(match: re.Match[str]) -> str:
        block = match.group(0)
        for url_match in URL_RE.finditer(block):
            url = url_match.group(0)
            if url not in DEPRECATED_QUICKSTART_ALLOWED_URLS:
                raise ValueError(
                    f"{relative_path}: deprecated quickstart marker contains unclassified URL: {url}"
                )
        return ""

    return DEPRECATED_QUICKSTART_MARKER_RE.sub(checked_replacement, body)


def has_freshness_status_guard(context: str) -> bool:
    lowered = context.lower()
    return (
        FRESHNESS_STATUS_MARKER_RE.search(context) is not None
        and all(word in lowered for word in FRESHNESS_STATUS_GUARD_WORDS)
        and any(word in lowered for word in FRESHNESS_STATUS_VALUES)
    )


def is_legacy_internal_reference(body: str, relative_path: Path, expected: dict[str, str]) -> bool:
    if str(relative_path) not in LEGACY_INTERNAL_DOCS:
        return False
    lines = [line.strip() for line in body.splitlines() if line.strip()]
    allowed = {
        expected["post-install-smoke"],
        expected["latest-install"],
        expected["pinned-install"],
    }
    return bool(lines) and all(line in allowed for line in lines)


def is_troubleshooting_pinned_install(body: str, context: str) -> bool:
    if CONCRETE_PINNED_INSTALL_RE.fullmatch(body.strip()) is None:
        return False
    lowered_context = context.lower()
    return any(word in lowered_context for word in TROUBLESHOOTING_CONTEXT_WORDS)


def is_install_status_command(body: str) -> bool:
    return body.strip() == "m80 install-status"


def is_rollback_cleanup_command(body: str, context: str) -> bool:
    lowered_context = context.lower()
    if "rollback" not in lowered_context and "cleanup" not in lowered_context:
        return False
    lines = [line.strip() for line in body.splitlines() if line.strip()]
    if not lines:
        return False
    allowed_prefixes = (
        "sudo ln -sfnT -- ",
        "m80 install-status",
        "sudo m80 install-cleanup ",
        "m80 install-cleanup ",
    )
    return all(line.startswith(allowed_prefixes) for line in lines)


def surrounding_context(lines: list[str], start_line: int, end_line: int) -> str:
    context_start = max(start_line - 8, 0)
    context_end = min(end_line + 3, len(lines))
    return "\n".join(lines[context_start:context_end])


def first_nonempty_line(body: str) -> str:
    for line in body.splitlines():
        stripped = line.strip()
        if stripped:
            return stripped
    return ""
