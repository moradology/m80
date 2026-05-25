#!/usr/bin/env bash
# Remove stale m80-owned host residue before a privileged E2E run.

set -euo pipefail

cd "$(dirname "$0")/.." || exit

exec python3 - "$@" <<'PY'
from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import shlex
import shutil
import subprocess
import sys
import time


OWNED_LINK = re.compile(r"^(tfc[0-9a-fA-F]{12}|brfc[0-9a-fA-F]{11}|bfc[0-9a-fA-F]{12}|m80-br.*)$")
OWNED_CHAIN = re.compile(r"^tfw[0-9a-fA-F]{12}$")
PROTECTED_RUN_DIR_NAMES = {".preserved", "warm", "templates"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-root", action="append", default=[])
    parser.add_argument(
        "--min-age-hours",
        type=float,
        default=float(os.environ.get("M80_E2E_REAP_MIN_AGE_HOURS", "1")),
    )
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


args = parse_args()
report: dict[str, object] = {
    "schema_version": 1,
    "dry_run": args.dry_run,
    "actions": [],
    "skipped": [],
    "errors": [],
}


def note(kind: str, target: str, command: list[str] | None = None) -> None:
    report["actions"].append({"kind": kind, "target": target, "command": command or []})


def skip(kind: str, target: str, reason: str) -> None:
    report["skipped"].append({"kind": kind, "target": target, "reason": reason})


def error(kind: str, target: str, detail: str) -> None:
    report["errors"].append({"kind": kind, "target": target, "detail": detail})


def privileged(command: list[str]) -> list[str]:
    if os.geteuid() == 0:
        return command
    return ["sudo", "-n", *command]


def run(command: list[str], *, needs_root: bool = False) -> subprocess.CompletedProcess[str]:
    actual = privileged(command) if needs_root else command
    return subprocess.run(actual, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def mutate(command: list[str], kind: str, target: str) -> None:
    actual = privileged(command)
    note(kind, target, actual)
    if args.dry_run:
        return
    proc = subprocess.run(actual, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if proc.returncode != 0:
        detail = proc.stderr.strip() or proc.stdout.strip() or f"exit {proc.returncode}"
        error(kind, target, detail)


def require_sudo_for_mutation() -> bool:
    if args.dry_run or os.geteuid() == 0:
        return True
    proc = subprocess.run(["sudo", "-n", "true"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if proc.returncode != 0:
        error("preflight", "sudo", "passwordless sudo is required for cleanup")
        return False
    return True


def reap_links() -> None:
    if shutil.which("ip") is None:
        skip("link", "ip", "ip command not found")
        return
    proc = run(["ip", "-o", "link", "show"])
    if proc.returncode != 0:
        error("link", "ip -o link show", proc.stderr.strip() or proc.stdout.strip())
        return
    for line in proc.stdout.splitlines():
        pieces = line.split(": ", 1)
        if len(pieces) != 2:
            continue
        name = pieces[1].split(":", 1)[0].split("@", 1)[0].strip()
        if OWNED_LINK.match(name):
            mutate(["ip", "link", "delete", name], "link", name)


def has_m80_comment(spec: list[str]) -> bool:
    return any(left == "--comment" and right.startswith("m80:") for left, right in zip(spec, spec[1:]))


def reap_iptables() -> None:
    if shutil.which("iptables") is None:
        skip("iptables", "iptables", "iptables command not found")
        return
    for table in ("filter", "nat"):
        proc = run(["iptables", "-w", "-t", table, "-S"], needs_root=True)
        if proc.returncode != 0:
            error("iptables", table, proc.stderr.strip() or proc.stdout.strip())
            continue
        chain_has_foreign_rule: dict[str, bool] = {}
        owned_chains: set[str] = set()
        owned_rules: list[tuple[str, list[str]]] = []
        for line in proc.stdout.splitlines():
            try:
                parts = shlex.split(line)
            except ValueError as exc:
                error("iptables", table, f"could not parse {line!r}: {exc}")
                continue
            if len(parts) == 2 and parts[0] == "-N" and OWNED_CHAIN.match(parts[1]):
                owned_chains.add(parts[1])
                continue
            if len(parts) < 3 or parts[0] != "-A":
                continue
            chain = parts[1]
            spec = parts[2:]
            if has_m80_comment(spec):
                owned_rules.append((chain, spec))
            elif OWNED_CHAIN.match(chain):
                chain_has_foreign_rule[chain] = True
        for chain, spec in owned_rules:
            mutate(["iptables", "-w", "-t", table, "-D", chain, *spec], "iptables-rule", f"{table}/{chain}")
        for chain in sorted(owned_chains):
            if chain_has_foreign_rule.get(chain):
                skip("iptables-chain", f"{table}/{chain}", "foreign rule present")
                continue
            mutate(["iptables", "-w", "-t", table, "-X", chain], "iptables-chain", f"{table}/{chain}")


def default_run_roots() -> list[pathlib.Path]:
    roots = args.run_root or [os.environ.get("M80_E2E_RUN_ROOT", "/var/lib/m80-r"), "/var/lib/m80-run"]
    seen: set[str] = set()
    out: list[pathlib.Path] = []
    for root in roots:
        path = pathlib.Path(root)
        key = str(path)
        if key not in seen:
            seen.add(key)
            out.append(path)
    return out


def safe_run_root(path: pathlib.Path) -> bool:
    text = str(path)
    if text in {"", "/", "/tmp", "/var", "/var/lib", "/tank", "/tank/tmp"}:
        return False
    return (
        text.startswith("/var/lib/m80")
        or text.startswith("/tank/tmp/m80")
        or text.startswith("/tmp/m80")
    )


def has_live_pid(path: pathlib.Path) -> bool:
    for pid_file in path.rglob("*.pid"):
        try:
            pid = int(pid_file.read_text().strip())
        except Exception:
            continue
        if pathlib.Path("/proc", str(pid)).exists():
            return True
    return False


def reap_run_roots() -> None:
    cutoff = time.time() - args.min_age_hours * 3600
    for root in default_run_roots():
        if not safe_run_root(root):
            error("run-root", str(root), "refusing unsafe run-root")
            continue
        if not root.exists():
            skip("run-root", str(root), "missing")
            continue
        for child in root.iterdir():
            if child.name in PROTECTED_RUN_DIR_NAMES or not child.is_dir() or child.is_symlink():
                continue
            try:
                mtime = child.stat().st_mtime
            except OSError as exc:
                error("run-dir", str(child), str(exc))
                continue
            if mtime > cutoff:
                skip("run-dir", str(child), "younger than min age")
                continue
            if has_live_pid(child):
                skip("run-dir", str(child), "live pid file")
                continue
            mutate(["rm", "-rf", str(child)], "run-dir", str(child))


def reap_cgroups() -> None:
    root = pathlib.Path("/sys/fs/cgroup/m80-firecracker")
    if not root.exists():
        skip("cgroup", str(root), "missing")
        return
    for child in root.iterdir():
        if not child.is_dir():
            continue
        procs = child / "cgroup.procs"
        try:
            text = procs.read_text().strip()
        except OSError as exc:
            error("cgroup", str(child), str(exc))
            continue
        if text:
            skip("cgroup", str(child), "live pids")
            continue
        mutate(["rmdir", str(child)], "cgroup", str(child))


if require_sudo_for_mutation():
    reap_links()
    reap_iptables()
    reap_run_roots()
    reap_cgroups()

if args.json:
    print(json.dumps(report, indent=2, sort_keys=True))
else:
    for item in report["actions"]:
        print(f"{item['kind']}: {item['target']}")
    for item in report["skipped"]:
        print(f"skip {item['kind']}: {item['target']} ({item['reason']})")
    for item in report["errors"]:
        print(f"error {item['kind']}: {item['target']}: {item['detail']}", file=sys.stderr)

sys.exit(1 if report["errors"] else 0)
PY
