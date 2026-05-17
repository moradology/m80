#!/usr/bin/env python3
"""Verify m80-q420k measurement close artifacts.

This is a close-time guard for the remaining measurement-shaped q420k beads.
It checks the artifact fields that the bead text names explicitly and can also
check that artifacts are committed and bead close reasons cite the verified
artifacts. It does not replace the required real-KVM run or the
`verified: <artifact> @ <commit>` close reason.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import io
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SNAPSHOT = ROOT / "crates/m80-firecracker/benches/snapshot_template_restore_latency.json"
DEFAULT_SNAPSHOT_DOC = ROOT / "docs/perf/snapshot-template-restore.md"
DEFAULT_DENSITY = ROOT / "docs/perf/pmem-shared-density.md"
DEFAULT_DENSITY_SMOKE = ROOT / "scripts/smoke-pmem-shared.sh"
DEFAULT_CLOSE_RUNBOOK = ROOT / "docs/runbook/q420k-close-gates.md"
DEFAULT_MEASUREMENT_PLAYBOOK = ROOT / "docs/perf/measurement-playbook.md"
DEFAULT_QUIET_HOST_INVENTORY = ROOT / "scripts/q420k-quiet-host-inventory.sh"
DEFAULT_COMPOSED_RESTORE = (
    ROOT / "crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json"
)
DEFAULT_COMPOSED_MEMORY = (
    ROOT / "crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json"
)
DEFAULT_COMPOSED_RESIDUE = (
    ROOT / "crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json"
)
DEFAULT_COMPOSED_DOC = ROOT / "docs/perf/composed-e2e.md"
DEFAULT_EXT4_OVERLAY = ROOT / "docs/perf/ext4-overlay-template-clone.md"
DEFAULT_DAX_MEMORY_PRESSURE = ROOT / "docs/perf/pmem-dax-memory-pressure.md"
SNAPSHOT_TEMPLATE_N_PER_RUN = 20
SNAPSHOT_TEMPLATE_RUNS = 3
SNAPSHOT_TEMPLATE_SAMPLES_TOTAL = SNAPSHOT_TEMPLATE_N_PER_RUN * SNAPSHOT_TEMPLATE_RUNS
COMPOSED_E2E_N = 10
COMPOSED_E2E_SHARED_PAYLOAD_MIB = 32
COMPOSED_E2E_OUT_DIR = "crates/m80-firecracker/benches/snapshots"
COMPOSED_E2E_RUN_ROOT = "/var/lib/m80-composed-e2e"
PREPARED_STRIPPED_KERNEL_IMAGE = (
    "/tank/projects/m80/crates/m80-image-build/kernels/"
    "vmlinux-m80-613988fdb6a6aaa0f806ec27e6f6e66875e2f768b28a2c0244aa4af3d8e2ac19.bin"
)
PREPARED_ROOTFS_IMAGE = "/tank/tmp/m80-build/post-restore-current/output.ext4"
PREPARED_STRIPPED_KERNEL_SHA256 = (
    "143b2784a434cdf5de10920a59e2c875b66be63bacfa9ba8ddf93ac60f2bc6e3"
)
PREPARED_ROOTFS_SHA256 = (
    "bfa35731760b9fbf06d41ffbfe500153d3247869dee75443dc836184b253613d"
)
PREPARED_FIRECRACKER_SHA256 = (
    "7e8b57e88c459396d4680d83dcdd8c7f72305447cb55b11f4ac98ad70a3f7825"
)
PREPARED_JAILER_SHA256 = (
    "4830a9b1fc6cece036d8992ff12f1fe9c5247aacad77f42c7aba683c7a08622e"
)
PREPARED_SECCOMP_SHA256 = (
    "bf0485c9e016e69d26c478c605c52a47a3ddf2b148749898031f42cf379228c0"
)
PREPARED_JAILER_HARDEN_SHA256 = (
    "bf01e083ea2ac36d73ed1ac00cd4dd3bde06b8a0c1b037efe88dd468696006ab"
)
PREPARED_NET_HELPER_SHA256 = (
    "09d732c1fda809d76f13e6b931ec196112afe81ac370b286a6801d4d745d4b3b"
)
REQUIRED_CLOSE_BEADS = {
    "snapshot-template": "m80-q420k.4.15",
    "pmem-density": "m80-q420k.3.8",
    "composed-restore": "m80-q420k.6.2",
    "composed-memory": "m80-q420k.6.3",
    "composed-residue": "m80-q420k.6.4",
    "composed-doc": "m80-q420k.6.5",
    "ext4-overlay": "m80-q420k.8.16",
    "dax-memory-pressure": "m80-q420k.8.9",
}
REQUIRED_PARENT_PHASES = [
    "m80-q420k.7",
    "m80-q420k.1",
    "m80-q420k.2",
    "m80-q420k.3",
    "m80-q420k.4",
    "m80-q420k.5",
    "m80-q420k.6",
]
REQUIRED_UNBLOCKED_ISSUES = {
    "m80-q420k",
    "m80-q420k.3",
    "m80-q420k.4",
    "m80-q420k.6",
}
COMPOSED_MEASUREMENT_CLOSE_BEADS = [
    (
        "m80-q420k.6.2",
        "crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json",
    ),
    (
        "m80-q420k.6.3",
        "crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json",
    ),
    (
        "m80-q420k.6.4",
        "crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json",
    ),
]
PARENT_PHASE_CLOSE_REFERENCES = {
    "m80-q420k.3": [
        ("m80-q420k.3.8", "docs/perf/pmem-shared-density.md"),
    ],
    "m80-q420k.4": [
        (
            "m80-q420k.4.15",
            "crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
        ),
    ],
    "m80-q420k.6": [
        ("m80-q420k.6.5", "docs/perf/composed-e2e.md"),
    ],
}
SUPER_EPIC_CLOSE_REFERENCE = ("m80-q420k.6", "docs/perf/composed-e2e.md")
REQUIRED_STRIPPED_KERNEL_LABELS = {
    "snapshot",
    "pmem density",
    "composed restore",
    "composed memory",
    "composed residue",
}
REQUIRED_CLOSE_ARTIFACT_PATHS = [
    DEFAULT_DENSITY,
    DEFAULT_SNAPSHOT,
    DEFAULT_COMPOSED_RESTORE,
    DEFAULT_COMPOSED_MEMORY,
    DEFAULT_COMPOSED_RESIDUE,
    DEFAULT_COMPOSED_DOC,
]


class Check:
    def __init__(self) -> None:
        self.errors: list[str] = []

    def require(self, condition: bool, message: str) -> None:
        if not condition:
            self.errors.append(message)

    def extend(self, errors: list[str]) -> None:
        self.errors.extend(errors)


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text())
    except FileNotFoundError as exc:
        raise AssertionError(f"missing artifact: {path}") from exc
    except json.JSONDecodeError as exc:
        raise AssertionError(f"invalid JSON in {path}: {exc}") from exc


def load_text(path: Path) -> str:
    try:
        return path.read_text()
    except FileNotFoundError as exc:
        raise AssertionError(f"missing artifact: {path}") from exc


def uncommented_text(text: str) -> str:
    return "\n".join(
        line for line in text.splitlines() if not line.lstrip().startswith("#")
    )


def artifact_path(raw: str | Path) -> Path:
    path = Path(raw)
    if not path.is_absolute():
        path = ROOT / path
    return path


def at(data: Any, dotted: str) -> Any:
    current = data
    for part in dotted.split("."):
        if not isinstance(current, dict) or part not in current:
            return None
        current = current[part]
    return current


def require_number_at_least(check: Check, label: str, value: Any, minimum: float) -> None:
    check.require(is_number(value), f"{label} must be numeric")
    if is_number(value):
        check.require(value >= minimum, f"{label} must be >= {minimum:g}")


def require_int_equal(check: Check, label: str, value: Any, expected: int) -> None:
    check.require(is_non_negative_int(value), f"{label} must be a non-negative integer")
    if is_non_negative_int(value):
        check.require(value == expected, f"{label} must equal {expected}")


def require_list_len(check: Check, label: str, value: Any, expected: Any) -> None:
    check.require(isinstance(value, list), f"{label} must be a list")
    if isinstance(value, list) and is_number(expected):
        check.require(
            len(value) == int(expected),
            f"{label} length must equal {int(expected)}",
        )


def require_positive_number_list(check: Check, label: str, value: Any) -> None:
    if not isinstance(value, list):
        check.require(False, f"{label} must be a list")
        return
    for index, item in enumerate(value):
        check.require(
            is_number(item) and item > 0,
            f"{label}[{index}] must be a positive number",
        )


def numeric_list(value: Any) -> list[float] | None:
    if not isinstance(value, list) or not all(is_number(item) for item in value):
        return None
    return [float(item) for item in value]


def percentile(values: list[float], pct: int) -> float:
    sorted_values = sorted(values)
    rank = ((len(sorted_values) * pct + 99) // 100) - 1
    return sorted_values[max(0, min(rank, len(sorted_values) - 1))]


def require_number_close(
    check: Check,
    label: str,
    actual: Any,
    expected: float,
    tolerance: float,
) -> None:
    check.require(is_number(actual), f"{label} must be numeric")
    if is_number(actual):
        check.require(
            abs(float(actual) - expected) <= tolerance,
            f"{label} {actual} must equal recomputed {expected:g}",
        )


def require_text_contains_number(
    check: Check,
    label: str,
    text: str,
    value: Any,
    formatted: str,
) -> None:
    check.require(is_number(value), f"{label} must be numeric")
    if is_number(value):
        check.require(formatted in text, f"{label} missing from text: {formatted}")


def require_git_commit(check: Check, label: str, value: Any) -> None:
    check.require(
        isinstance(value, str) and re.fullmatch(r"[0-9a-f]{40}", value) is not None,
        f"{label}: git_commit must be a full 40-character commit sha",
    )


def require_sha256_hex(check: Check, label: str, value: Any) -> None:
    check.require(
        isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None,
        f"{label}: must be a 64-character lowercase sha256 hex digest",
    )


def quiet_substrate(data: Any, label: str, check: Check) -> None:
    substrate = at(data, "substrate")
    check.require(isinstance(substrate, dict), f"{label}: missing substrate object")
    if not isinstance(substrate, dict):
        return
    check.require(
        substrate.get("substrate_kind") == "real-kvm",
        f"{label}: substrate.substrate_kind must be real-kvm",
    )
    check.require(
        substrate.get("preflight_required") is True,
        f"{label}: substrate.preflight_required must be true",
    )
    check.require(
        substrate.get("quiet_host_checked") is True,
        f"{label}: substrate.quiet_host_checked must be true",
    )
    check.require(
        substrate.get("allow_other_firecracker_vms") is False,
        f"{label}: substrate.allow_other_firecracker_vms must be false",
    )
    check.require(
        substrate.get("preexisting_firecracker_processes") == [],
        f"{label}: substrate.preexisting_firecracker_processes must be empty",
    )
    check.require(
        substrate.get("post_run_firecracker_processes") == [],
        f"{label}: substrate.post_run_firecracker_processes must be empty",
    )
    verify_preflight_artifacts(substrate.get("preflight_artifacts"), label, check)


def runtime_substrate(data: Any, label: str, check: Check) -> None:
    substrate = at(data, "substrate")
    if not isinstance(substrate, dict):
        return
    host_kernel = substrate.get("host_kernel_release")
    check.require(
        kernel_version_at_least(host_kernel, 6, 5),
        f"{label}: substrate.host_kernel_release must be >= 6.5",
    )
    firecracker_version = substrate.get("firecracker_version")
    expected_version = at(data, "substrate.preflight_artifacts.expected_firecracker_version")
    check.require(
        isinstance(firecracker_version, str)
        and isinstance(expected_version, str)
        and expected_version in firecracker_version,
        f"{label}: substrate.firecracker_version must include expected Firecracker version",
    )
    dev_kvm_stat = substrate.get("dev_kvm_stat")
    check.require(
        isinstance(dev_kvm_stat, str) and "/dev/kvm" in dev_kvm_stat and "rw" in dev_kvm_stat,
        f"{label}: substrate.dev_kvm_stat must prove /dev/kvm is rw",
    )
    check.require(
        substrate.get("sudo_uid") == "0",
        f"{label}: substrate.sudo_uid must be 0",
    )


def require_composed_run_id(check: Check, label: str, data: Any) -> None:
    run_id = data.get("run_id") if isinstance(data, dict) else None
    check.require(
        isinstance(run_id, str) and re.fullmatch(r"composed-[0-9a-f]{4,}", run_id) is not None,
        f"{label}: run_id must identify the composed test invocation",
    )


def verify_preflight_artifacts(artifacts: Any, label: str, check: Check) -> None:
    check.require(isinstance(artifacts, dict), f"{label}: missing substrate.preflight_artifacts object")
    if not isinstance(artifacts, dict):
        return
    for field in [
        "firecracker_bin",
        "firecracker_seccomp_filter",
        "jailer_bin",
        "jailer_harden_bin",
        "net_helper_bin",
        "kernel_image",
        "rootfs_image",
    ]:
        value = artifacts.get(field)
        check.require(
            isinstance(value, str) and value.startswith("/"),
            f"{label}: substrate.preflight_artifacts.{field} must be an absolute path",
        )
    for field in ["kernel_image_sha256", "rootfs_image_sha256"]:
        value = artifacts.get(field)
        check.require(
            isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None,
            f"{label}: substrate.preflight_artifacts.{field} must be a sha256 hex digest",
        )
    check.require(
        artifacts.get("kernel_kind") in {"stock", "stripped"},
        f"{label}: substrate.preflight_artifacts.kernel_kind must be stock or stripped",
    )
    if label in REQUIRED_STRIPPED_KERNEL_LABELS:
        check.require(
            artifacts.get("kernel_kind") == "stripped",
            f"{label}: substrate.preflight_artifacts.kernel_kind must be stripped",
        )
    check.require(
        artifacts.get("image_kind") in {"ubuntu", "minimal"},
        f"{label}: substrate.preflight_artifacts.image_kind must be ubuntu or minimal",
    )
    check.require(
        artifacts.get("rootfs_format") in {"ext4", "erofs"},
        f"{label}: substrate.preflight_artifacts.rootfs_format must be ext4 or erofs",
    )
    check.require(
        isinstance(artifacts.get("expected_firecracker_version"), str)
        and artifacts.get("expected_firecracker_version") != "",
        f"{label}: substrate.preflight_artifacts.expected_firecracker_version must be non-empty",
    )


def verify_snapshot_template(path: Path) -> list[str]:
    data = load_json(path)
    check = Check()
    check.require(data.get("schema_version") == 1, "snapshot: schema_version must be 1")
    check.require(
        data.get("bench") == "snapshot_template_restore_latency",
        "snapshot: bench must be snapshot_template_restore_latency",
    )
    check.require(data.get("load") == "idle", "snapshot: load must be idle")
    check.require(data.get("target_ready") == 1, "snapshot: target_ready must be 1")
    n_per_run = data.get("n_per_run")
    runs = data.get("runs")
    samples_total = data.get("samples_total")
    check.require(
        is_non_negative_int(n_per_run),
        "snapshot: n_per_run must be a non-negative integer",
    )
    check.require(
        is_non_negative_int(runs),
        "snapshot: runs must be a non-negative integer",
    )
    check.require(
        is_non_negative_int(samples_total),
        "snapshot: samples_total must be a non-negative integer",
    )
    check.require(
        n_per_run == SNAPSHOT_TEMPLATE_N_PER_RUN,
        f"snapshot: n_per_run must equal {SNAPSHOT_TEMPLATE_N_PER_RUN}",
    )
    check.require(
        runs == SNAPSHOT_TEMPLATE_RUNS,
        f"snapshot: runs must equal {SNAPSHOT_TEMPLATE_RUNS}",
    )
    check.require(
        samples_total == SNAPSHOT_TEMPLATE_SAMPLES_TOTAL,
        f"snapshot: samples_total must equal {SNAPSHOT_TEMPLATE_SAMPLES_TOTAL}",
    )
    if (
        is_non_negative_int(n_per_run)
        and is_non_negative_int(runs)
        and is_non_negative_int(samples_total)
    ):
        expected_total = n_per_run * runs
        check.require(
            samples_total == expected_total,
            f"snapshot: samples_total must equal n_per_run * runs ({expected_total})",
        )
    samples = at(data, "data.warm.samples_us")
    sample_details = at(data, "data.warm.sample_details")
    require_list_len(check, "snapshot: data.warm.samples_us", samples, data.get("samples_total"))
    require_list_len(
        check,
        "snapshot: data.warm.sample_details",
        sample_details,
        data.get("samples_total"),
    )
    require_positive_number_list(check, "snapshot: data.warm.samples_us", samples)
    if isinstance(sample_details, list):
        observed_run_cycles = set()
        for index, detail in enumerate(sample_details):
            check.require(
                isinstance(detail, dict),
                f"snapshot: data.warm.sample_details[{index}] must be an object",
            )
            if isinstance(detail, dict):
                for field in ["fill_us", "handoff_us", "restore_to_handback_us"]:
                    check.require(
                        is_number(detail.get(field)) and detail.get(field) >= 0,
                        f"snapshot: data.warm.sample_details[{index}].{field} must be numeric",
                    )
                run = detail.get("run")
                cycle = detail.get("cycle")
                check.require(
                    is_non_negative_int(run),
                    f"snapshot: data.warm.sample_details[{index}].run must be a non-negative integer",
                )
                check.require(
                    is_non_negative_int(cycle),
                    f"snapshot: data.warm.sample_details[{index}].cycle must be a non-negative integer",
                )
                if is_non_negative_int(run) and is_non_negative_int(cycle):
                    observed_run_cycles.add((run, cycle))
                fill_us = detail.get("fill_us")
                handoff_us = detail.get("handoff_us")
                restore_us = detail.get("restore_to_handback_us")
                if is_number(fill_us) and is_number(handoff_us) and is_number(restore_us):
                    check.require(
                        fill_us + handoff_us == restore_us,
                        f"snapshot: data.warm.sample_details[{index}] fill_us + handoff_us must equal restore_to_handback_us",
                    )
                if (
                    isinstance(samples, list)
                    and index < len(samples)
                    and is_number(samples[index])
                    and is_number(restore_us)
                ):
                    check.require(
                        samples[index] == restore_us,
                        f"snapshot: data.warm.sample_details[{index}].restore_to_handback_us must match samples_us[{index}]",
                    )
        if is_non_negative_int(runs) and is_non_negative_int(n_per_run):
            expected_run_cycles = {
                (run, cycle)
                for run in range(runs)
                for cycle in range(n_per_run)
            }
            check.require(
                observed_run_cycles == expected_run_cycles,
                "snapshot: sample_details must cover each run/cycle exactly once",
            )
    runs_detail = data.get("runs_detail")
    require_list_len(check, "snapshot: runs_detail", runs_detail, data.get("runs"))
    if isinstance(runs_detail, list):
        for index, run_detail in enumerate(runs_detail):
            check.require(
                isinstance(run_detail, dict),
                f"snapshot: runs_detail[{index}] must be an object",
            )
            if not isinstance(run_detail, dict):
                continue
            check.require(
                run_detail.get("run") == index,
                f"snapshot: runs_detail[{index}].run must equal {index}",
            )
            run_samples = run_detail.get("samples_us")
            require_list_len(
                check,
                f"snapshot: runs_detail[{index}].samples_us",
                run_samples,
                data.get("n_per_run"),
            )
            require_positive_number_list(
                check,
                f"snapshot: runs_detail[{index}].samples_us",
                run_samples,
            )
    numeric_samples = numeric_list(samples)
    if numeric_samples:
        expected_p99_us = percentile(numeric_samples, 99)
        require_number_close(
            check,
            "snapshot: data.warm.restore_to_handback_us.p99",
            at(data, "data.warm.restore_to_handback_us.p99"),
            expected_p99_us,
            0.0,
        )
        require_number_close(
            check,
            "snapshot: data.warm.restore_to_handback_ms.p99",
            at(data, "data.warm.restore_to_handback_ms.p99"),
            expected_p99_us / 1000.0,
            0.001,
        )
    require_number_at_least(check, "snapshot: vcpu_count", data.get("vcpu_count"), 1)
    require_number_at_least(check, "snapshot: mem_size_mib", data.get("mem_size_mib"), 1)
    require_number_at_least(check, "snapshot: jail_uid", data.get("jail_uid"), 1)
    require_number_at_least(check, "snapshot: jail_gid", data.get("jail_gid"), 1)
    check.require(data.get("cgroup_mode") == "disabled", "snapshot: cgroup_mode must be disabled")
    check.require(
        data.get("page_cache_dropped_between_samples") is True,
        "snapshot: page_cache_dropped_between_samples must be true",
    )
    check.require(
        data.get("git_worktree_dirty_excluding_artifact") is False,
        "snapshot: measured source worktree must be clean except for the artifact",
    )
    require_git_commit(check, "snapshot", data.get("git_commit"))
    p99 = at(data, "data.warm.restore_to_handback_ms.p99")
    check.require(is_number(p99), "snapshot: data.warm.restore_to_handback_ms.p99 missing")
    if is_number(p99):
        check.require(p99 <= 200.0, f"snapshot: p99 {p99} exceeds 200 ms")
    quiet_substrate(data, "snapshot", check)
    runtime_substrate(data, "snapshot", check)
    require_snapshot_reproduction_command(data, check)
    return check.errors


def require_snapshot_reproduction_command(data: dict[str, Any], check: Check) -> None:
    command = data.get("reproduction_command")
    check.require(isinstance(command, str) and command, "snapshot: missing reproduction_command")
    if not isinstance(command, str):
        return
    for required in [
        "M80_RUN_ROOT=",
        "cargo bench -p m80-firecracker --bench snapshot_template_restore_latency",
    ]:
        check.require(required in command, f"snapshot: reproduction_command missing {required}")
    for env, expected in [
        ("M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS", 0),
        ("M80_SNAPSHOT_BENCH_LOAD", "idle"),
        (
            "M80_SNAPSHOT_TEMPLATE_BENCH_OUTPUT",
            "crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
        ),
        ("M80_KERNEL_KIND", "stripped"),
        ("M80_CGROUP_MODE", "disabled"),
    ]:
        require_command_env_value(check, "snapshot", command, env, expected)
    require_command_env_value(check, "snapshot", command, "N", SNAPSHOT_TEMPLATE_N_PER_RUN)
    require_command_env_value(
        check,
        "snapshot",
        command,
        "M80_SNAPSHOT_TEMPLATE_RUNS",
        SNAPSHOT_TEMPLATE_RUNS,
    )
    for env, field in [
        ("M80_SNAPSHOT_BENCH_VCPU_COUNT", "vcpu_count"),
        ("M80_SNAPSHOT_BENCH_MEM_SIZE_MIB", "mem_size_mib"),
        ("M80_JAIL_UID", "jail_uid"),
        ("M80_JAIL_GID", "jail_gid"),
    ]:
        value = data.get(field)
        if is_number(value):
            require_command_env_value(check, "snapshot", command, env, int(value))
    preflight = at(data, "substrate.preflight_artifacts")
    if isinstance(preflight, dict):
        for env, field in [
            ("M80_FIRECRACKER_BIN", "firecracker_bin"),
            ("M80_JAILER_BIN", "jailer_bin"),
            ("M80_FIRECRACKER_SECCOMP_FILTER", "firecracker_seccomp_filter"),
            ("M80_JAILER_HARDEN_BIN", "jailer_harden_bin"),
            ("M80_NET_HELPER_BIN", "net_helper_bin"),
            ("M80_KERNEL_IMAGE", "kernel_image"),
            ("M80_ROOTFS_IMAGE", "rootfs_image"),
        ]:
            value = preflight.get(field)
            if isinstance(value, str):
                require_command_env_value(check, "snapshot", command, env, value)


def require_command_env_value(
    check: Check,
    label: str,
    command: str,
    env: str,
    expected: Any,
) -> None:
    value = command_env_value(command, env)
    check.require(
        value == str(expected),
        f"{label}: reproduction_command must set {env}={expected}",
    )


def require_command_env_present(
    check: Check,
    label: str,
    command: str,
    env: str,
) -> str | None:
    value = command_env_value(command, env)
    check.require(
        value is not None and value != "",
        f"{label}: command must set {env}",
    )
    return value


def require_command_env_absolute_path(
    check: Check,
    label: str,
    command: str,
    env: str,
) -> str | None:
    value = require_command_env_present(check, label, command, env)
    if value is not None:
        check.require(value.startswith("/"), f"{label}: {env} must be an absolute path")
    return value


def command_env_value(command: str, env: str) -> str | None:
    match = re.search(rf"(?:^|\s){re.escape(env)}=(?P<value>[^\s\\]+)", command)
    if match is None:
        return None
    value = match.group("value")
    if len(value) >= 2 and value[0] == "'" and value[-1] == "'":
        return value[1:-1].replace("'\\''", "'")
    return value


def verify_snapshot_doc(path: Path) -> list[str]:
    text = load_text(path)
    check = Check()
    identity_section = markdown_section(text, "Artifact identity")
    command_section = markdown_section(text, "Command")
    command = markdown_shell_block(command_section) if command_section is not None else None
    check.require(
        text.startswith("# Snapshot-Template Restore Latency"),
        "snapshot doc: title mismatch",
    )
    for required in [
        "crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
        "data.warm.restore_to_handback_ms.p99",
        "target_ready=1",
        "M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS=0",
        "M80_SNAPSHOT_BENCH_LOAD=idle",
        "M80_SNAPSHOT_BENCH_VCPU_COUNT=",
        "M80_SNAPSHOT_BENCH_MEM_SIZE_MIB=",
        "M80_KERNEL_KIND=stripped",
        "M80_RUN_ROOT=",
        "M80_JAIL_UID=",
        "M80_JAIL_GID=",
        "M80_CGROUP_MODE=disabled",
        "N=20 M80_SNAPSHOT_TEMPLATE_RUNS=3",
        "M80_SNAPSHOT_TEMPLATE_BENCH_OUTPUT=crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
        "cargo bench -p m80-firecracker --bench snapshot_template_restore_latency",
        "verified: crates/m80-firecracker/benches/snapshot_template_restore_latency.json @ <commit-sha>",
        "python3 scripts/verify-q420k-artifacts.py --only snapshot-template --require-committed",
        "git_commit",
        "reproduction_command",
        "substrate.preflight_artifacts",
        "substrate.post_run_firecracker_processes",
    ]:
        check.require(required in text, f"snapshot doc: missing {required}")
    check.require(identity_section is not None, "snapshot doc: missing ## Artifact identity")
    check.require(command is not None, "snapshot doc: missing ## Command shell block")
    if command is not None:
        for required in [
            "sync && echo 3 | sudo tee /proc/sys/vm/drop_caches",
            "sudo -n env",
            "cargo bench -p m80-firecracker --bench snapshot_template_restore_latency",
        ]:
            check.require(required in command, f"snapshot doc command: missing {required}")
        for env, expected in [
            ("M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS", 0),
            ("M80_SNAPSHOT_BENCH_LOAD", "idle"),
            ("M80_SNAPSHOT_TEMPLATE_BENCH_OUTPUT", "crates/m80-firecracker/benches/snapshot_template_restore_latency.json"),
            ("M80_KERNEL_KIND", "stripped"),
            ("M80_CGROUP_MODE", "disabled"),
            ("N", SNAPSHOT_TEMPLATE_N_PER_RUN),
            ("M80_SNAPSHOT_TEMPLATE_RUNS", SNAPSHOT_TEMPLATE_RUNS),
        ]:
            require_command_env_value(check, "snapshot doc command", command, env, expected)
        for env in [
            "M80_FIRECRACKER_BIN",
            "M80_JAILER_BIN",
            "M80_FIRECRACKER_SECCOMP_FILTER",
            "M80_JAILER_HARDEN_BIN",
            "M80_NET_HELPER_BIN",
            "M80_KERNEL_IMAGE",
            "M80_ROOTFS_IMAGE",
            "M80_RUN_ROOT",
        ]:
            require_command_env_absolute_path(check, "snapshot doc command", command, env)
        for env in [
            "M80_SNAPSHOT_BENCH_VCPU_COUNT",
            "M80_SNAPSHOT_BENCH_MEM_SIZE_MIB",
            "M80_JAIL_UID",
            "M80_JAIL_GID",
        ]:
            value = require_command_env_present(check, "snapshot doc command", command, env)
            if value is not None:
                check.require(
                    re.fullmatch(r"\d+", value) is not None,
                    f"snapshot doc command: {env} must be an integer",
                )
    smoke = markdown_section(text, "Smoke evidence")
    check.require(smoke is not None, "snapshot doc: missing ## Smoke evidence")
    if smoke is not None:
        smoke_lines = re.findall(r"^snapshot-template restore: .*$", smoke, flags=re.MULTILINE)
        check.require(
            len(smoke_lines) == 1,
            "snapshot doc: smoke evidence must contain exactly one snapshot-template restore line",
        )
        for required in [
            "snapshot-template restore: load=idle runs=3 n=20",
            "p99=",
            "output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
        ]:
            check.require(required in smoke, f"snapshot doc: smoke evidence missing {required}")
        if smoke_lines:
            check.require(
                re.fullmatch(
                    r"snapshot-template restore: load=idle runs=3 n=20 "
                    r"p99=\d+us "
                    r"output=crates/m80-firecracker/benches/snapshot_template_restore_latency\.json",
                    smoke_lines[0],
                )
                is not None,
                "snapshot doc: smoke evidence line must be the canonical close-quality shape",
            )
    for marker in [
        "Pending quiet-host close run",
        "## Diagnostic Run",
        "This does not close `m80-q420k.4.15`",
        "unrelated `t2-warm-slot-*` Firecracker processes",
    ]:
        check.require(marker not in text, f"snapshot doc: diagnostic marker remains: {marker}")
    return check.errors


def verify_snapshot_doc_consistency(doc_path: Path, snapshot_path: Path) -> list[str]:
    text = load_text(doc_path)
    data = load_json(snapshot_path)
    check = Check()
    identity_section = markdown_section(text, "Artifact identity")
    identity_text = identity_section or ""
    check.require(identity_section is not None, "snapshot doc: missing ## Artifact identity")
    command_section = markdown_section(text, "Command")
    command = markdown_shell_block(command_section) if command_section is not None else None
    smoke = markdown_section(text, "Smoke evidence")
    commit = data.get("git_commit")
    if isinstance(commit, str):
        check.require(
            commit in identity_text,
            "snapshot doc: Artifact identity section missing measured git_commit from JSON artifact",
        )
    for field in [
        "host_kernel_release",
        "firecracker_version",
        "dev_kvm_stat",
        "sudo_uid",
    ]:
        value = at(data, f"substrate.{field}")
        if isinstance(value, str) and value:
            check.require(
                value in identity_text,
                f"snapshot doc: Artifact identity section missing "
                f"substrate.{field} from JSON artifact",
            )
    p99_us = at(data, "data.warm.restore_to_handback_us.p99")
    p99_ms = at(data, "data.warm.restore_to_handback_ms.p99")
    if is_number(p99_us) or is_number(p99_ms):
        p99_us = int(p99_us if is_number(p99_us) else round(p99_ms * 1000))
        check.require(
            f"p99={p99_us}us" in (smoke or ""),
            "snapshot doc: smoke p99 does not match JSON artifact",
        )
    preflight = at(data, "substrate.preflight_artifacts")
    if isinstance(preflight, dict):
        for field in [
            "firecracker_bin",
            "firecracker_seccomp_filter",
            "jailer_bin",
            "jailer_harden_bin",
            "net_helper_bin",
            "kernel_image",
            "rootfs_image",
            "kernel_image_sha256",
            "rootfs_image_sha256",
            "expected_firecracker_version",
        ]:
            value = preflight.get(field)
            if isinstance(value, str) and value:
                check.require(
                    value in identity_text,
                    f"snapshot doc: Artifact identity section missing "
                    f"substrate.preflight_artifacts.{field} from JSON artifact",
                )
        if command is not None:
            for env, field in [
                ("M80_FIRECRACKER_BIN", "firecracker_bin"),
                ("M80_JAILER_BIN", "jailer_bin"),
                ("M80_FIRECRACKER_SECCOMP_FILTER", "firecracker_seccomp_filter"),
                ("M80_JAILER_HARDEN_BIN", "jailer_harden_bin"),
                ("M80_NET_HELPER_BIN", "net_helper_bin"),
                ("M80_KERNEL_IMAGE", "kernel_image"),
                ("M80_ROOTFS_IMAGE", "rootfs_image"),
            ]:
                value = preflight.get(field)
                if isinstance(value, str) and value:
                    require_command_env_value(check, "snapshot doc command", command, env, value)
    if command is not None:
        for env, field in [
            ("M80_SNAPSHOT_BENCH_VCPU_COUNT", "vcpu_count"),
            ("M80_SNAPSHOT_BENCH_MEM_SIZE_MIB", "mem_size_mib"),
            ("M80_JAIL_UID", "jail_uid"),
            ("M80_JAIL_GID", "jail_gid"),
        ]:
            value = data.get(field)
            if is_number(value):
                require_command_env_value(check, "snapshot doc command", command, env, int(value))
    return check.errors


def verify_pmem_density(path: Path) -> list[str]:
    text = load_text(path)
    check = Check()
    check.require(text.startswith("# Shared pmem density"), "pmem density: title mismatch")
    reproduction_section = markdown_section(text, "Reproduction") or ""
    substrate_section = markdown_section(text, "Substrate") or ""
    observable_section = markdown_section(text, "Observable") or ""
    teardown_section = markdown_section(text, "Teardown") or ""
    layout_section = markdown_section(text, "Payload erofs layout") or ""
    trust_section = markdown_section(text, "Trust model") or ""
    check.require(reproduction_section != "", "pmem density: missing Reproduction section")
    check.require(substrate_section != "", "pmem density: missing Substrate section")
    check.require(observable_section != "", "pmem density: missing Observable section")
    check.require(teardown_section != "", "pmem density: missing Teardown section")
    command = markdown_text(reproduction_section, r"Command: `([^`]+)`")
    host_kernel = markdown_text(substrate_section, r"- host kernel: `([^`]+)`")
    firecracker_version = markdown_text(substrate_section, r"- firecracker: `([^`]+)`")
    kvm_stat = markdown_text(substrate_section, r"- `/dev/kvm`: `([^`]+)`")
    sudo_uid = markdown_text(substrate_section, r"- sudo: required; test ran as uid `([^`]+)`")
    vm_count = markdown_int(observable_section, r"- field: host memory delta after (\d+) attached Shared VMs")
    cycles = markdown_int(observable_section, r"- cycles: `(\d+)`")
    payload_mib = markdown_int(observable_section, r"- payload size: `(\d+) MiB`")
    image_digest = markdown_text(observable_section, r"- image digest: `([^`]+)`")
    image_path = markdown_text(observable_section, r"- image path: `([^`]+)`")
    layout = markdown_int(observable_section, r"- payload erofs layout: `Layout: (\d+)`,")
    payload_size = markdown_int(observable_section, r"size `(\d+)` bytes, on-disk size")
    payload_on_disk = markdown_int(observable_section, r"on-disk size `(\d+)` bytes")
    image_kib = markdown_int(observable_section, r"- image KiB: `(\d+)`")
    per_vm_overhead = markdown_int(observable_section, r"- per-VM overhead bound: `(\d+) KiB`")
    bound = markdown_int(observable_section, r"- bound: `(\d+) KiB`")
    max_delta = markdown_int(observable_section, r"- max observed delta: `(\d+) KiB`")
    result = markdown_text(observable_section, r"- result: `([^`]+)`")
    max_active_markers = markdown_int(teardown_section, r"- max active-use markers observed: `(\d+)`")
    final_active_markers = markdown_int(teardown_section, r"- final active-use markers: `(\d+)`")
    stale_swept = markdown_int(teardown_section, r"- stale markers swept after teardown: `(\d+)`")
    canonical_present = markdown_text(
        teardown_section,
        r"- canonical Shared artifact present after teardown: `([^`]+)`",
    )
    samples = pmem_density_samples(text)

    check.require(
        command is not None,
        "pmem density: missing reproduction command",
    )
    if command is not None:
        try:
            artifact_rel = path.resolve().relative_to(ROOT).as_posix()
        except ValueError:
            artifact_rel = path.as_posix()
        for env, expected in [
            ("M80_PMEM_SHARED_ALLOW_OTHER_VMS", 0),
            ("M80_PMEM_SHARED_DENSITY_ARTIFACT", artifact_rel),
            ("M80_KERNEL_KIND", "stripped"),
        ]:
            require_command_env_value(check, "pmem density", command, env, expected)
        for required in [
            "M80_PMEM_SHARED_VM_COUNT=",
            "M80_PMEM_SHARED_CYCLES=",
            "M80_PMEM_SHARED_PAYLOAD_MIB=",
            "M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=",
            "M80_RUN_ROOT=",
            "M80_KERNEL_IMAGE=",
            "M80_ROOTFS_IMAGE=",
            "M80_FIRECRACKER_BIN=",
            "M80_JAILER_BIN=",
            "M80_FIRECRACKER_SECCOMP_FILTER=",
            "M80_JAILER_HARDEN_BIN=",
            "M80_NET_HELPER_BIN=",
            "M80_FIRECRACKER_VERSION=",
            "M80_JAIL_UID=",
            "M80_JAIL_GID=",
            "scripts/smoke-pmem-shared.sh",
        ]:
            check.require(required in command, f"pmem density: reproduction command missing {required}")
        for env, value in [
            ("M80_PMEM_SHARED_VM_COUNT", vm_count),
            ("M80_PMEM_SHARED_CYCLES", cycles),
            ("M80_PMEM_SHARED_PAYLOAD_MIB", payload_mib),
            ("M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB", per_vm_overhead),
        ]:
            if is_number(value):
                require_command_env_value(check, "pmem density", command, env, int(value))

    check.require(
        markdown_text(substrate_section, r"- git worktree dirty excluding this artifact: `([^`]+)`") == "false",
        "pmem density: measured source worktree must be clean except for the artifact",
    )
    require_git_commit(check, "pmem density", markdown_text(substrate_section, r"- git commit: `([^`]+)`"))
    check.require(
        kernel_version_at_least(host_kernel, 6, 5),
        "pmem density: host kernel must be recorded and >= 6.5",
    )
    check.require(
        isinstance(firecracker_version, str) and firecracker_version,
        "pmem density: Firecracker version output must be recorded",
    )
    check.require(
        isinstance(kvm_stat, str) and "/dev/kvm" in kvm_stat and "rw" in kvm_stat,
        "pmem density: /dev/kvm rw stat must be recorded",
    )
    check.require(sudo_uid == "0", "pmem density: sudo/root uid evidence must be 0")
    require_number_at_least(check, "pmem density: vm_count", vm_count, 4)
    require_number_at_least(check, "pmem density: cycles", cycles, 10)
    require_number_at_least(check, "pmem density: payload MiB", payload_mib, 1)
    require_sha256_hex(check, "pmem density: image digest", image_digest)
    check.require(
        isinstance(image_path, str) and image_path.startswith("/"),
        "pmem density: image path must be absolute",
    )
    if isinstance(image_path, str) and isinstance(image_digest, str):
        check.require(
            image_digest in image_path,
            "pmem density: image path must contain the measured image digest",
        )
    check.require(layout == 0, "pmem density: payload erofs layout must be 0")
    require_number_at_least(check, "pmem density: payload size", payload_size, 1)
    require_number_at_least(check, "pmem density: payload on-disk size", payload_on_disk, 1)
    if is_number(payload_size) and is_number(payload_on_disk):
        check.require(
            payload_size == payload_on_disk,
            "pmem density: payload logical and on-disk size must match",
        )
    require_number_at_least(check, "pmem density: image KiB", image_kib, 1)
    require_number_at_least(check, "pmem density: per-VM overhead KiB", per_vm_overhead, 1)
    require_number_at_least(check, "pmem density: bound KiB", bound, 1)
    if is_number(bound) and is_number(image_kib) and is_number(per_vm_overhead) and is_number(vm_count):
        expected_bound = image_kib + per_vm_overhead * vm_count
        check.require(
            bound == expected_bound,
            f"pmem density: bound {bound} must equal image_kib + per_vm_overhead * vm_count ({expected_bound})",
        )
    check.require(is_number(max_delta), "pmem density: max observed delta KiB must be numeric")
    if is_number(max_delta) and is_number(bound):
        check.require(max_delta <= bound, f"pmem density: max delta {max_delta} exceeds bound {bound}")
    check.require(samples is not None, "pmem density: missing Samples table")
    if samples is not None:
        if is_number(cycles):
            check.require(
                len(samples) == cycles,
                f"pmem density: Samples row count must equal cycles ({cycles})",
            )
            check.require(
                [sample["cycle"] for sample in samples] == list(range(1, int(cycles) + 1)),
                "pmem density: Samples cycles must cover 1..cycles exactly",
            )
        if is_number(bound):
            for sample in samples:
                check.require(
                    sample["bound_kib"] == bound,
                    f"pmem density: sample {sample['cycle']} bound must equal summary bound",
                )
                check.require(
                    sample["delta_kib"] <= bound,
                    f"pmem density: sample {sample['cycle']} delta exceeds bound",
                )
        for sample in samples:
            expected_delta = max(0, sample["before_kib"] - sample["after_kib"])
            check.require(
                sample["delta_kib"] == expected_delta,
                f"pmem density: sample {sample['cycle']} delta must equal before-after",
            )
        if is_number(max_delta) and samples:
            check.require(
                max(sample["delta_kib"] for sample in samples) == max_delta,
                "pmem density: max observed delta must equal max Samples delta",
            )
    check.require(result == "pass", "pmem density: result must be pass")
    check.require(
        max_active_markers == vm_count,
        "pmem density: max active-use markers observed must equal vm_count",
    )
    check.require(final_active_markers == 0, "pmem density: final active-use markers must be 0")
    check.require(stale_swept == 0, "pmem density: stale markers swept after teardown must be 0")
    check.require(
        canonical_present == "true",
        "pmem density: canonical Shared artifact must remain after teardown",
    )
    check.require(
        "dropped page cache before each cycle" in substrate_section,
        "pmem density: missing page-cache-drop substrate statement",
    )
    check.require(
        "The measured file is required to match the `.8.12` file-level DAX result" in layout_section,
        "pmem density: missing .8.12 layout interpretation",
    )
    for required in [
        "TrustDomainAck",
        "same trust domain",
        "DAX cache-timing side channel",
        "Shared pmem jail bindings are read-only",
    ]:
        check.require(required in trust_section, f"pmem density: missing trust-model statement: {required}")

    substrate = markdown_json_block(substrate_section, "Firecracker process substrate")
    if substrate is None:
        check.require(False, "pmem density: missing Firecracker process substrate JSON block")
    else:
        quiet_substrate({"substrate": substrate}, "pmem density", check)
        runtime_substrate({"substrate": substrate}, "pmem density", check)
        preflight = substrate.get("preflight_artifacts")
        if command is not None and isinstance(preflight, dict):
            expected_version = preflight.get("expected_firecracker_version")
            if isinstance(expected_version, str) and isinstance(firecracker_version, str):
                check.require(
                    expected_version in firecracker_version,
                    "pmem density: Firecracker version output must match preflight version",
                )
            for env, field in [
                ("M80_FIRECRACKER_BIN", "firecracker_bin"),
                ("M80_JAILER_BIN", "jailer_bin"),
                ("M80_FIRECRACKER_SECCOMP_FILTER", "firecracker_seccomp_filter"),
                ("M80_JAILER_HARDEN_BIN", "jailer_harden_bin"),
                ("M80_NET_HELPER_BIN", "net_helper_bin"),
                ("M80_KERNEL_IMAGE", "kernel_image"),
                ("M80_ROOTFS_IMAGE", "rootfs_image"),
                ("M80_FIRECRACKER_VERSION", "expected_firecracker_version"),
                ("M80_KERNEL_KIND", "kernel_kind"),
            ]:
                value = preflight.get(field)
                if isinstance(value, str):
                    require_command_env_value(check, "pmem density", command, env, value)
    return check.errors


def verify_pmem_density_smoke(path: Path) -> list[str]:
    text = load_text(path)
    code_text = uncommented_text(text)
    check = Check()
    check.require(path.is_file(), "pmem density smoke script: path must be a file")
    check.require(
        path.stat().st_mode & 0o111 != 0,
        "pmem density smoke script: script must be executable",
    )
    check.require(
        "#!/usr/bin/env bash" in text,
        "pmem density smoke script: missing #!/usr/bin/env bash",
    )
    for required in [
        "set -euo pipefail",
        "M80_PMEM_SHARED_VM_COUNT:-4",
        "M80_PMEM_SHARED_CYCLES:-10",
        "M80_PMEM_SHARED_PAYLOAD_MIB:-128",
        "M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB:-131072",
        'RUN_ROOT="${M80_RUN_ROOT:-/var/lib/m80-psd}"',
        "M80_PMEM_SHARED_DENSITY_ARTIFACT:-docs/perf/pmem-shared-density.md",
        "M80_PMEM_SHARED_ALLOW_OTHER_VMS",
        'FIRECRACKER_BIN="${M80_FIRECRACKER_BIN:-/opt/firecracker/bin/firecracker}"',
        'JAILER_BIN="${M80_JAILER_BIN:-/opt/firecracker/bin/jailer}"',
        "M80_FIRECRACKER_SECCOMP_FILTER:-/opt/firecracker/bin/firecracker-seccomp-filter.bin",
        "M80_JAILER_HARDEN_BIN:-/opt/m80/bin/m80-jailer-harden",
        "M80_NET_HELPER_BIN:-/opt/m80/bin/m80-net-helper",
        'if [[ "$ALLOW_OTHER_VMS" != "1" ]]; then',
        "pgrep -af",
        "refusing density measurement while other Firecracker VMs are present",
        "exit 3",
        "sudo -n true",
        "cargo test -p m80-firecracker --test pmem_shared_host_page_sharing_real_kvm --no-run",
        'repro_command="M80_PMEM_SHARED_ALLOW_OTHER_VMS=$ALLOW_OTHER_VMS"',
        "repro_command+=\" M80_PMEM_SHARED_VM_COUNT=$VM_COUNT\"",
        "repro_command+=\" M80_PMEM_SHARED_CYCLES=$CYCLES\"",
        "repro_command+=\" M80_PMEM_SHARED_PAYLOAD_MIB=$PAYLOAD_MIB\"",
        "repro_command+=\" M80_FIRECRACKER_SECCOMP_FILTER=$FIRECRACKER_SECCOMP_FILTER\"",
        "repro_command+=\" M80_JAILER_HARDEN_BIN=$JAILER_HARDEN_BIN\"",
        "repro_command+=\" M80_NET_HELPER_BIN=$NET_HELPER_BIN\"",
        "repro_command+=\" M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=$PER_VM_OVERHEAD_KIB\"",
        "repro_command+=\" M80_PMEM_SHARED_DENSITY_ARTIFACT=$ARTIFACT\"",
        "repro_command+=\" M80_RUN_ROOT=$RUN_ROOT\"",
        "repro_command+=\" M80_KERNEL_IMAGE=$KERNEL_IMAGE\"",
        "repro_command+=\" M80_KERNEL_KIND=$KERNEL_KIND\"",
        "repro_command+=\" M80_ROOTFS_IMAGE=$ROOTFS_IMAGE\"",
        "repro_command+=\" M80_FIRECRACKER_BIN=$FIRECRACKER_BIN\"",
        "repro_command+=\" M80_JAILER_BIN=$JAILER_BIN\"",
        "repro_command+=\" M80_FIRECRACKER_VERSION=$FIRECRACKER_VERSION\"",
        "repro_command+=\" M80_JAIL_UID=$JAIL_UID\"",
        "repro_command+=\" M80_JAIL_GID=$JAIL_GID\"",
        "repro_command+=\" $0\"",
        "timeout 1800 sudo -n env",
        "M80_RUN_PMEM_SHARED_DENSITY=1",
        "M80_PMEM_SHARED_VM_COUNT=\"$VM_COUNT\"",
        "M80_PMEM_SHARED_CYCLES=\"$CYCLES\"",
        "M80_PMEM_SHARED_PAYLOAD_MIB=\"$PAYLOAD_MIB\"",
        "M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=\"$PER_VM_OVERHEAD_KIB\"",
        "M80_PMEM_SHARED_ALLOW_OTHER_VMS=\"$ALLOW_OTHER_VMS\"",
        "M80_PMEM_SHARED_DENSITY_ARTIFACT=\"$ARTIFACT_PATH\"",
        "M80_PMEM_SHARED_REPRO_COMMAND=\"$repro_command\"",
        "M80_FIRECRACKER_BIN=\"$FIRECRACKER_BIN\"",
        "M80_JAILER_BIN=\"$JAILER_BIN\"",
        "M80_FIRECRACKER_SECCOMP_FILTER=\"$FIRECRACKER_SECCOMP_FILTER\"",
        "M80_JAILER_HARDEN_BIN=\"$JAILER_HARDEN_BIN\"",
        "M80_NET_HELPER_BIN=\"$NET_HELPER_BIN\"",
        "M80_KERNEL_IMAGE=\"$KERNEL_IMAGE\"",
        "M80_KERNEL_KIND=\"$KERNEL_KIND\"",
        "M80_ROOTFS_IMAGE=\"$ROOTFS_IMAGE\"",
        "M80_RUN_ROOT=\"$RUN_ROOT\"",
        "M80_FIRECRACKER_VERSION=\"$FIRECRACKER_VERSION\"",
        "M80_JAIL_UID=\"$JAIL_UID\"",
        "M80_JAIL_GID=\"$JAIL_GID\"",
        "shared_pmem_host_page_sharing_measurement_lives_in_density_gate",
        "--ignored --exact --nocapture",
    ]:
        check.require(
            required in code_text,
            f"pmem density smoke script: missing executable code {required}",
        )
    return check.errors


def verify_pmem_density_instruction_doc(path: Path) -> list[str]:
    text = load_text(path)
    check = Check()
    section = markdown_section(text, "1. Shared Pmem Density")
    if section is None:
        section = markdown_section(text, "Q420K Shared Pmem Density")
    command = markdown_shell_block(section) if section is not None else None
    check.require(path.is_file(), f"pmem density instruction doc: missing {path}")
    check.require(section is not None, "pmem density instruction doc: missing Shared Pmem Density section")
    check.require(command is not None, "pmem density instruction doc: missing command shell block")
    for required in [
        "M80_PMEM_SHARED_ALLOW_OTHER_VMS=0",
        "M80_PMEM_SHARED_VM_COUNT=4",
        "M80_PMEM_SHARED_CYCLES=10",
        "M80_PMEM_SHARED_PAYLOAD_MIB=128",
        "M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=131072",
        "M80_PMEM_SHARED_DENSITY_ARTIFACT=docs/perf/pmem-shared-density.md",
        "M80_RUN_ROOT=/var/lib/m80-psd",
        "M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker",
        "M80_JAILER_BIN=/opt/firecracker/bin/jailer",
        "M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin",
        "M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden",
        "M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper",
        f"M80_KERNEL_IMAGE={PREPARED_STRIPPED_KERNEL_IMAGE}",
        "M80_KERNEL_KIND=stripped",
        f"M80_ROOTFS_IMAGE={PREPARED_ROOTFS_IMAGE}",
        "M80_FIRECRACKER_VERSION=v1.15.1",
        "M80_JAIL_UID=",
        "M80_JAIL_GID=",
        "./scripts/smoke-pmem-shared.sh",
    ]:
        check.require(
            command is not None and required in command,
            f"pmem density instruction command: missing {required}",
        )
    for required in [
        "python3 scripts/verify-q420k-artifacts.py --only pmem-density --require-committed",
    ]:
        check.require(
            section is not None and required in section,
            f"pmem density instruction doc: missing {required}",
        )
    if path.name == "q420k-close-gates.md":
        verify_prepared_close_input_text(text, check)
    return check.errors


def verify_prepared_close_input_text(text: str, check: Check) -> None:
    for required in [
        "Prepared `vulcan` close inputs",
        PREPARED_STRIPPED_KERNEL_IMAGE,
        PREPARED_STRIPPED_KERNEL_SHA256,
        PREPARED_ROOTFS_IMAGE,
        PREPARED_ROOTFS_SHA256,
        "/opt/firecracker/bin/firecracker",
        PREPARED_FIRECRACKER_SHA256,
        "Firecracker v1.15.1",
        "/opt/firecracker/bin/jailer",
        PREPARED_JAILER_SHA256,
        "Jailer v1.15.1",
        "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
        PREPARED_SECCOMP_SHA256,
        "/opt/m80/bin/m80-jailer-harden",
        PREPARED_JAILER_HARDEN_SHA256,
        "/opt/m80/bin/m80-net-helper",
        PREPARED_NET_HELPER_SHA256,
        "M80_KERNEL_KIND=stripped",
        "`m80-preflight`",
        "applies that override to the emitted `preflight_artifacts`",
    ]:
        check.require(
            required in text,
            f"prepared close inputs: missing {required}",
        )


def verify_quiet_host_inventory(path: Path) -> list[str]:
    text = load_text(path)
    check = Check()
    check.require(path.is_file(), "quiet-host inventory helper: path must be a file")
    check.require(
        path.stat().st_mode & 0o111 != 0,
        "quiet-host inventory helper: script must be executable",
    )
    for required in [
        "#!/usr/bin/env bash",
        "set -euo pipefail",
        "pgrep -x firecracker",
        "kubectl get pod -A -o json",
        "systemctl --user status",
        "Refusing close-quality q420k measurement",
        "This script is inventory only",
        "exit 1",
    ]:
        check.require(required in text, f"quiet-host inventory helper: missing {required}")
    for forbidden in [
        r"\bkill\s+-?[0-9]",
        r"\bpkill\b",
        r"\bkillall\b",
        r"\bkubectl\s+(delete|drain|scale|patch|cordon|taint)\b",
        r"\bsystemctl\s+(--user\s+)?(stop|kill|restart)\b",
    ]:
        check.require(
            re.search(forbidden, text) is None,
            f"quiet-host inventory helper: contains forbidden mutating pattern {forbidden}",
        )
    return check.errors


def verify_composed_restore(path: Path) -> list[str]:
    data = load_json(path)
    check = Check()
    check.require(data.get("schema_version") == 1, "composed restore: schema_version must be 1")
    check.require(
        data.get("scenario") == "composed_e2e_layered_warm_pool",
        "composed restore: scenario mismatch",
    )
    require_composed_run_id(check, "composed restore", data)
    check.require(
        data.get("git_worktree_dirty_excluding_artifacts") is False,
        "composed restore: measured source worktree must be clean except for artifacts",
    )
    require_git_commit(check, "composed restore", data.get("git_commit"))
    check.require(
        data.get("page_cache_dropped_between_leases") is False,
        "composed restore: page cache must not be dropped between leases",
    )
    restore = at(data, "data.restore_latency")
    check.require(isinstance(restore, dict), "composed restore: missing restore_latency")
    if isinstance(restore, dict):
        require_int_equal(check, "composed restore: count", restore.get("count"), COMPOSED_E2E_N)
        require_int_equal(
            check,
            "composed restore: target_ready",
            restore.get("target_ready"),
            COMPOSED_E2E_N,
        )
        if is_number(restore.get("count")) and is_number(restore.get("target_ready")):
            check.require(
                restore.get("target_ready") == restore.get("count"),
                "composed restore: target_ready must equal count",
            )
        samples_ms = restore.get("samples_ms")
        require_list_len(check, "composed restore: samples_ms", samples_ms, restore.get("count"))
        require_positive_number_list(check, "composed restore: samples_ms", samples_ms)
        numeric_samples = numeric_list(samples_ms)
        if numeric_samples:
            for pct in [50, 95, 99]:
                require_number_close(
                    check,
                    f"composed restore: p{pct}_ms",
                    restore.get(f"p{pct}_ms"),
                    percentile(numeric_samples, pct),
                    0.001,
                )
        template_build_warmup_ms = restore.get("template_build_warmup_ms")
        check.require(
            isinstance(template_build_warmup_ms, list) and template_build_warmup_ms,
            "composed restore: template_build_warmup_ms must be a non-empty list",
        )
        if isinstance(template_build_warmup_ms, list):
            require_positive_number_list(
                check,
                "composed restore: template_build_warmup_ms",
                template_build_warmup_ms,
            )
        check.require(restore.get("fail_count") == 0, "composed restore: fail_count must be 0")
        p99 = restore.get("p99_ms")
        check.require(is_number(p99), "composed restore: p99_ms missing")
        if is_number(p99):
            check.require(p99 <= 200.0, f"composed restore: p99_ms {p99} exceeds 200")
    warm_pool = at(data, "data.warm_pool")
    check.require(isinstance(warm_pool, dict), "composed restore: missing warm_pool")
    if isinstance(warm_pool, dict):
        after_fill = warm_pool.get("after_fill")
        check.require(isinstance(after_fill, dict), "composed restore: warm_pool.after_fill missing")
        if isinstance(after_fill, dict):
            require_int_equal(
                check,
                "composed restore: warm_pool.after_fill.target_ready",
                after_fill.get("target_ready"),
                COMPOSED_E2E_N,
            )
            require_int_equal(
                check,
                "composed restore: warm_pool.after_fill.ready",
                after_fill.get("ready"),
                COMPOSED_E2E_N,
            )
            check.require(
                after_fill.get("leased") == 0,
                "composed restore: warm_pool.after_fill.leased must be 0",
            )
            check.require(
                after_fill.get("filling") == 0,
                "composed restore: warm_pool.after_fill.filling must be 0",
            )
            check.require(
                after_fill.get("fill_failures_total") == 0,
                "composed restore: warm_pool.after_fill.fill_failures_total must be 0",
            )
        during_leases = warm_pool.get("during_leases")
        check.require(isinstance(during_leases, dict), "composed restore: warm_pool.during_leases missing")
        if isinstance(during_leases, dict):
            require_int_equal(
                check,
                "composed restore: warm_pool.during_leases.target_ready",
                during_leases.get("target_ready"),
                COMPOSED_E2E_N,
            )
            require_int_equal(
                check,
                "composed restore: warm_pool.during_leases.leased",
                during_leases.get("leased"),
                COMPOSED_E2E_N,
            )
            check.require(
                during_leases.get("ready") == 0,
                "composed restore: warm_pool.during_leases.ready must be 0",
            )
            check.require(
                during_leases.get("filling") == 0,
                "composed restore: warm_pool.during_leases.filling must be 0",
            )
            check.require(
                during_leases.get("fill_failures_total") == 0,
                "composed restore: warm_pool.during_leases.fill_failures_total must be 0",
            )
        check.require(
            isinstance(warm_pool.get("after_discard"), dict),
            "composed restore: warm_pool.after_discard missing",
        )
    observability = at(data, "data.observability")
    check.require(isinstance(observability, dict), "composed restore: missing observability")
    if isinstance(observability, dict):
        check.require(
            observability.get("pmem_layers_by_sharing") == {"Shared": 1, "PerVm": 1},
            "composed restore: observability.pmem_layers_by_sharing must be Shared=1 and PerVm=1",
        )
        diagnostics = observability.get("diagnostics")
        require_list_len(
            check,
            "composed restore: observability.diagnostics",
            diagnostics,
            COMPOSED_E2E_N,
        )
        if isinstance(diagnostics, list):
            required_phases = [
                "phase_3_storage_prep",
                "phase_restore_load",
                "phase_restore_probe_exec_channel",
                "phase_restore_post_restore_hooks",
            ]
            required_messages = ["snapshot restored", "restored guestd ready"]
            for index, summary in enumerate(diagnostics):
                check.require(
                    isinstance(summary, dict),
                    f"composed restore: diagnostics[{index}] must be an object",
                )
                if not isinstance(summary, dict):
                    continue
                check.require(
                    isinstance(summary.get("run_dir"), str)
                    and summary.get("run_dir").startswith("/"),
                    f"composed restore: diagnostics[{index}].run_dir must be absolute",
                )
                check.require(
                    isinstance(summary.get("vm_id"), str) and summary.get("vm_id"),
                    f"composed restore: diagnostics[{index}].vm_id must be non-empty",
                )
                phases = summary.get("phase_completed")
                check.require(
                    isinstance(phases, list),
                    f"composed restore: diagnostics[{index}].phase_completed must be a list",
                )
                if isinstance(phases, list):
                    for phase in required_phases:
                        check.require(
                            phase in phases,
                            f"composed restore: diagnostics[{index}] missing phase {phase}",
                        )
                messages = summary.get("lifecycle_messages")
                check.require(
                    isinstance(messages, list),
                    f"composed restore: diagnostics[{index}].lifecycle_messages must be a list",
                )
                if isinstance(messages, list):
                    for message in required_messages:
                        check.require(
                            message in messages,
                            f"composed restore: diagnostics[{index}] missing lifecycle message {message}",
                        )
                require_number_at_least(
                    check,
                    f"composed restore: diagnostics[{index}].exec_completed_count",
                    summary.get("exec_completed_count"),
                    5,
                )
    quiet_substrate(data, "composed restore", check)
    runtime_substrate(data, "composed restore", check)
    return check.errors


def verify_composed_memory(path: Path) -> list[str]:
    data = load_json(path)
    check = Check()
    check.require(data.get("schema_version") == 1, "composed memory: schema_version must be 1")
    check.require(
        data.get("scenario") == "composed_e2e_layered_warm_pool",
        "composed memory: scenario mismatch",
    )
    require_composed_run_id(check, "composed memory", data)
    check.require(
        data.get("git_worktree_dirty_excluding_artifacts") is False,
        "composed memory: measured source worktree must be clean except for artifacts",
    )
    require_git_commit(check, "composed memory", data.get("git_commit"))
    check.require(
        data.get("page_cache_dropped_between_fill_and_attach") is False,
        "composed memory: page cache must not be dropped between fill and attach checkpoint",
    )
    memory = at(data, "data.host_memory")
    check.require(isinstance(memory, dict), "composed memory: missing host_memory")
    if isinstance(memory, dict):
        require_int_equal(check, "composed memory: n_attached", memory.get("n_attached"), COMPOSED_E2E_N)
        check.require(
            memory.get("per_vm_overhead_source") == "per_vm_baseline_same_run",
            "composed memory: per_vm_overhead_source must be per_vm_baseline_same_run",
        )
        baseline = memory.get("baseline_before_fill_bytes")
        after_attached = memory.get("after_n_attached_bytes")
        shared_image_path = memory.get("shared_image_path")
        delta = memory.get("after_n_attached_delta_bytes")
        bound = memory.get("bound_bytes")
        shared_image_bytes = memory.get("shared_image_bytes")
        per_vm_overhead_bytes = memory.get("per_vm_overhead_bytes")
        shared_image_digest = memory.get("shared_image_digest")
        for field, value in [
            ("baseline_before_fill_bytes", baseline),
            ("after_fill_bytes", memory.get("after_fill_bytes")),
            ("after_n_attached_bytes", after_attached),
            ("after_teardown_bytes", memory.get("after_teardown_bytes")),
        ]:
            check.require(is_number(value) and value > 0, f"composed memory: {field} must be > 0")
        check.require(is_number(delta), "composed memory: after_n_attached_delta_bytes missing")
        check.require(is_number(bound), "composed memory: bound_bytes missing")
        check.require(is_number(shared_image_bytes), "composed memory: shared_image_bytes missing")
        check.require(is_number(per_vm_overhead_bytes), "composed memory: per_vm_overhead_bytes missing")
        require_sha256_hex(check, "composed memory: shared_image_digest", shared_image_digest)
        check.require(
            isinstance(shared_image_path, str) and shared_image_path.startswith("/"),
            "composed memory: shared_image_path must be absolute",
        )
        if isinstance(shared_image_path, str) and isinstance(shared_image_digest, str):
            check.require(
                shared_image_digest in shared_image_path,
                "composed memory: shared_image_path must contain shared_image_digest",
            )
        check.require(is_number(memory.get("shared_image_dev")), "composed memory: shared_image_dev missing")
        check.require(is_number(memory.get("shared_image_ino")), "composed memory: shared_image_ino missing")
        if is_number(baseline) and is_number(after_attached) and is_number(delta):
            check.require(
                baseline - after_attached == delta,
                "composed memory: after_n_attached_delta_bytes must equal baseline_before_fill_bytes - after_n_attached_bytes",
            )
        per_vm_baseline = memory.get("per_vm_baseline")
        check.require(isinstance(per_vm_baseline, dict), "composed memory: missing per_vm_baseline")
        if isinstance(per_vm_baseline, dict):
            baseline_n_attached = per_vm_baseline.get("n_attached")
            check.require(
                baseline_n_attached == memory.get("n_attached"),
                "composed memory: per_vm_baseline.n_attached must equal n_attached",
            )
            for field in [
                "before_fill_bytes",
                "after_fill_bytes",
                "after_n_attached_bytes",
                "after_teardown_bytes",
                "after_n_attached_delta_bytes",
                "per_vm_payload_bytes",
                "per_vm_overhead_bytes",
                "shared_payload_bytes",
            ]:
                value = per_vm_baseline.get(field)
                check.require(
                    is_number(value) and value >= 0,
                    f"composed memory: per_vm_baseline.{field} must be >= 0",
                )
            check.require(
                per_vm_baseline.get("per_vm_overhead_bytes") == per_vm_overhead_bytes,
                "composed memory: per_vm_baseline.per_vm_overhead_bytes must match top-level per_vm_overhead_bytes",
            )
            check.require(
                per_vm_baseline.get("shared_payload_digest") == shared_image_digest,
                "composed memory: per_vm_baseline.shared_payload_digest must equal shared_image_digest",
            )
            check.require(
                per_vm_baseline.get("shared_payload_bytes") == shared_image_bytes,
                "composed memory: per_vm_baseline.shared_payload_bytes must equal shared_image_bytes",
            )
            require_sha256_hex(
                check,
                "composed memory: per_vm_baseline.image_digest",
                per_vm_baseline.get("image_digest"),
            )
            check.require(
                is_number(per_vm_baseline.get("image_bytes")) and per_vm_baseline.get("image_bytes") > 0,
                "composed memory: per_vm_baseline.image_bytes must be > 0",
            )
            fingerprint = per_vm_baseline.get("template_fingerprint")
            check.require(
                isinstance(fingerprint, str) and re.fullmatch(r"[0-9a-f]{64}", fingerprint) is not None,
                "composed memory: per_vm_baseline.template_fingerprint must be a 64-character lowercase hex string",
            )
            if is_number(shared_image_bytes) and is_number(memory.get("n_attached")):
                expected_payload = shared_image_bytes * memory["n_attached"]
                check.require(
                    per_vm_baseline.get("per_vm_payload_bytes") == expected_payload,
                    f"composed memory: per_vm_baseline.per_vm_payload_bytes must equal shared_image_bytes * n_attached ({expected_payload})",
                )
            baseline_before = per_vm_baseline.get("before_fill_bytes")
            baseline_after_attached = per_vm_baseline.get("after_n_attached_bytes")
            baseline_delta = per_vm_baseline.get("after_n_attached_delta_bytes")
            baseline_payload = per_vm_baseline.get("per_vm_payload_bytes")
            if is_number(baseline_before) and is_number(baseline_after_attached) and is_number(baseline_delta):
                check.require(
                    baseline_before - baseline_after_attached == baseline_delta,
                    "composed memory: per_vm_baseline.after_n_attached_delta_bytes must equal before_fill_bytes - after_n_attached_bytes",
                )
            if is_number(baseline_delta) and is_number(baseline_payload):
                check.require(
                    baseline_delta >= baseline_payload,
                    "composed memory: per_vm_baseline delta must cover per_vm_payload_bytes",
                )
            if (
                is_number(baseline_delta)
                and is_number(baseline_payload)
                and is_number(baseline_n_attached)
                and baseline_n_attached > 0
            ):
                expected_overhead = max(0, baseline_delta - baseline_payload)
                expected_overhead = (expected_overhead + baseline_n_attached - 1) // baseline_n_attached
                check.require(
                    per_vm_baseline.get("per_vm_overhead_bytes") == expected_overhead,
                    f"composed memory: per_vm_baseline.per_vm_overhead_bytes must equal ceil((delta - payload) / n_attached) ({expected_overhead})",
                )
            attached_snapshot = per_vm_baseline.get("attached_snapshot")
            check.require(
                isinstance(attached_snapshot, dict),
                "composed memory: per_vm_baseline.attached_snapshot missing",
            )
            if isinstance(attached_snapshot, dict):
                check.require(
                    attached_snapshot.get("target_ready") == baseline_n_attached,
                    "composed memory: per_vm_baseline.attached_snapshot.target_ready must equal n_attached",
                )
                check.require(
                    attached_snapshot.get("leased") == baseline_n_attached,
                    "composed memory: per_vm_baseline.attached_snapshot.leased must equal n_attached",
                )
                check.require(
                    attached_snapshot.get("ready") == 0,
                    "composed memory: per_vm_baseline.attached_snapshot.ready must be 0",
                )
                check.require(
                    attached_snapshot.get("filling") == 0,
                    "composed memory: per_vm_baseline.attached_snapshot.filling must be 0",
                )
                check.require(
                    attached_snapshot.get("fill_failures_total") == 0,
                    "composed memory: per_vm_baseline.attached_snapshot.fill_failures_total must be 0",
                )
        if (
            is_number(bound)
            and is_number(shared_image_bytes)
            and is_number(per_vm_overhead_bytes)
            and is_number(memory.get("n_attached"))
        ):
            expected_bound = shared_image_bytes + per_vm_overhead_bytes * memory["n_attached"]
            check.require(
                bound == expected_bound,
                f"composed memory: bound_bytes {bound} must equal shared_image_bytes + per_vm_overhead_bytes * n_attached ({expected_bound})",
            )
        if is_number(delta) and is_number(bound):
            check.require(delta <= bound, f"composed memory: delta {delta} exceeds bound {bound}")
        check.require(memory.get("bound_satisfied") is True, "composed memory: bound_satisfied must be true")
        layout = memory.get("shared_payload_layout")
        check.require(isinstance(layout, dict), "composed memory: missing shared_payload_layout")
        if isinstance(layout, dict):
            check.require(layout.get("erofs_layout") == 0, "composed memory: erofs_layout must be 0")
            size = layout.get("size_bytes")
            on_disk = layout.get("on_disk_size_bytes")
            check.require(is_number(size) and size > 0, "composed memory: layout size_bytes must be > 0")
            check.require(
                is_number(on_disk) and on_disk > 0,
                "composed memory: layout on_disk_size_bytes must be > 0",
            )
            if is_number(size) and is_number(on_disk):
                check.require(
                    size == on_disk,
                    "composed memory: layout logical and on-disk size must match",
                )
    quiet_substrate(data, "composed memory", check)
    runtime_substrate(data, "composed memory", check)
    return check.errors


def verify_composed_residue(path: Path) -> list[str]:
    data = load_json(path)
    check = Check()
    check.require(data.get("schema_version") == 1, "composed residue: schema_version must be 1")
    check.require(
        data.get("scenario") == "composed_e2e_layered_warm_pool",
        "composed residue: scenario mismatch",
    )
    require_composed_run_id(check, "composed residue", data)
    check.require(
        data.get("git_worktree_dirty_excluding_artifacts") is False,
        "composed residue: measured source worktree must be clean except for artifacts",
    )
    require_git_commit(check, "composed residue", data.get("git_commit"))
    residue = at(data, "data.residue")
    check.require(isinstance(residue, dict), "composed residue: missing residue")
    if isinstance(residue, dict):
        require_int_equal(check, "composed residue: n_leases", residue.get("n_leases"), COMPOSED_E2E_N)
        check.require(residue.get("unexpected_paths") == [], "composed residue: unexpected_paths must be []")
        roots = residue.get("scanned_roots")
        dynamic_run_roots: list[str] = []
        check.require(isinstance(roots, list) and roots, "composed residue: scanned_roots must be non-empty")
        if isinstance(roots, list):
            root_strings = [root for root in roots if isinstance(root, str)]
            check.require(
                len(root_strings) >= 5,
                "composed residue: scanned_roots must cover run root, /tmp, /var/run, image store, and template store",
            )
            for required in ["/tmp/m80-*", "/var/run/m80", "/var/lib/m80-images"]:
                check.require(required in root_strings, f"composed residue: scanned_roots missing {required}")
            fixed_roots = {"/tmp/m80-*", "/var/run/m80", "/var/lib/m80-images"}
            dynamic_run_roots = [
                root
                for root in root_strings
                if root.startswith("/") and root not in fixed_roots and not root.endswith("/templates")
            ]
            check.require(
                bool(dynamic_run_roots),
                "composed residue: scanned_roots missing run root",
            )
            check.require(
                any(root.endswith("/templates") for root in root_strings),
                "composed residue: scanned_roots missing template store root",
            )
        leased_run_dirs = residue.get("leased_run_dirs")
        if isinstance(leased_run_dirs, list):
            check.require(
                len(leased_run_dirs) >= residue.get("n_leases", 0),
                "composed residue: leased_run_dirs must enumerate every lease",
            )
            for index, run_dir in enumerate(leased_run_dirs):
                check.require(
                    isinstance(run_dir, str) and run_dir.startswith("/"),
                    f"composed residue: leased_run_dirs[{index}] must be an absolute path string",
                )
                if isinstance(run_dir, str) and dynamic_run_roots:
                    check.require(
                        any(
                            run_dir == run_root or run_dir.startswith(f"{run_root}/")
                            for run_root in dynamic_run_roots
                        ),
                        f"composed residue: leased_run_dirs[{index}] must live under a scanned run root",
                    )
        else:
            check.require(False, "composed residue: leased_run_dirs must be a list")
        image_store = residue.get("image_store")
        check.require(isinstance(image_store, dict), "composed residue: missing image_store")
        if isinstance(image_store, dict):
            for field in ["shared_digest", "per_vm_digest"]:
                require_sha256_hex(
                    check,
                    f"composed residue: image_store.{field}",
                    image_store.get(field),
                )
            for field in ["expected", "preserved"]:
                digests = image_store.get(field)
                check.require(
                    isinstance(digests, list) and digests,
                    f"composed residue: image_store.{field} must be a non-empty list",
                )
                if isinstance(digests, list):
                    for digest in digests:
                        require_sha256_hex(check, f"composed residue: image_store.{field} digest", digest)
            check.require(
                sorted(image_store.get("preserved", [])) == sorted(image_store.get("expected", [])),
                "composed residue: image_store preserved must equal expected",
            )
            expected_images = image_store.get("expected")
            shared_digest = image_store.get("shared_digest")
            per_vm_digest = image_store.get("per_vm_digest")
            if (
                isinstance(expected_images, list)
                and isinstance(shared_digest, str)
                and isinstance(per_vm_digest, str)
            ):
                check.require(
                    sorted(expected_images) == sorted([shared_digest, per_vm_digest]),
                    "composed residue: image_store expected must be exactly Shared and PerVm digests",
                )
        template_store = residue.get("template_store")
        check.require(isinstance(template_store, dict), "composed residue: missing template_store")
        if isinstance(template_store, dict):
            expected = template_store.get("expected_fingerprint")
            preserved = template_store.get("preserved")
            require_sha256_hex(
                check,
                "composed residue: template_store.expected_fingerprint",
                expected,
            )
            check.require(
                isinstance(preserved, list) and preserved,
                "composed residue: template_store.preserved must be a non-empty list",
            )
            if isinstance(preserved, list):
                for fingerprint in preserved:
                    require_sha256_hex(
                        check,
                        "composed residue: template_store.preserved fingerprint",
                        fingerprint,
                    )
            check.require(
                sorted(preserved) == [expected] if isinstance(preserved, list) else False,
                "composed residue: template_store preserved must equal expected_fingerprint",
            )
    quiet_substrate(data, "composed residue", check)
    runtime_substrate(data, "composed residue", check)
    return check.errors


def verify_composed_consistency(
    restore_path: Path,
    memory_path: Path,
    residue_path: Path,
) -> list[str]:
    restore = load_json(restore_path)
    memory = load_json(memory_path)
    residue = load_json(residue_path)
    check = Check()

    restore_commit = restore.get("git_commit")
    memory_commit = memory.get("git_commit")
    residue_commit = residue.get("git_commit")
    check.require(
        restore_commit == memory_commit == residue_commit,
        "composed consistency: restore, memory, and residue artifacts must share git_commit",
    )
    restore_run_id = restore.get("run_id")
    memory_run_id = memory.get("run_id")
    residue_run_id = residue.get("run_id")
    check.require(
        restore_run_id == memory_run_id == residue_run_id,
        "composed consistency: restore, memory, and residue artifacts must share run_id",
    )
    check.require(
        restore.get("substrate") == memory.get("substrate") == residue.get("substrate"),
        "composed consistency: restore, memory, and residue artifacts must share identical substrate",
    )

    target_ready = at(restore, "data.restore_latency.target_ready")
    restore_count = at(restore, "data.restore_latency.count")
    n_attached = at(memory, "data.host_memory.n_attached")
    n_leases = at(residue, "data.residue.n_leases")
    check.require(
        target_ready == restore_count == n_attached == n_leases,
        "composed consistency: target_ready, restore count, n_attached, and n_leases must match",
    )
    check.require(
        target_ready == COMPOSED_E2E_N,
        f"composed consistency: target_ready must equal {COMPOSED_E2E_N}",
    )

    memory_shared = at(memory, "data.host_memory.shared_image_digest")
    residue_shared = at(residue, "data.residue.image_store.shared_digest")
    require_sha256_hex(check, "composed consistency: Shared image digest", memory_shared)
    check.require(
        isinstance(memory_shared, str) and memory_shared == residue_shared,
        "composed consistency: memory and residue Shared image digests must match",
    )
    image_expected = at(residue, "data.residue.image_store.expected")
    image_preserved = at(residue, "data.residue.image_store.preserved")
    if isinstance(image_expected, list) and isinstance(image_preserved, list) and isinstance(memory_shared, str):
        check.require(
            memory_shared in image_expected and memory_shared in image_preserved,
            "composed consistency: Shared image digest must be expected and preserved",
        )
    return check.errors


def verify_composed_command(
    check: Check,
    label: str,
    command: str,
    *,
    kernel_image: str | None = None,
    rootfs_image: str | None = None,
) -> None:
    for required in [
        "sudo -n env",
        "cargo test --release -p m80-firecracker --test e2e_composed_real_kvm",
        "--ignored composed_e2e_layered_warm_pool --nocapture",
    ]:
        check.require(required in command, f"{label}: command missing {required}")
    for env, expected in [
        ("M80_COMPOSED_E2E_ALLOW_OTHER_VMS", 0),
        ("M80_COMPOSED_E2E_N", COMPOSED_E2E_N),
        ("M80_COMPOSED_E2E_SHARED_PAYLOAD_MIB", COMPOSED_E2E_SHARED_PAYLOAD_MIB),
        ("M80_COMPOSED_E2E_OUT_DIR", f"{ROOT}/{COMPOSED_E2E_OUT_DIR}"),
        ("M80_RUN_ROOT", COMPOSED_E2E_RUN_ROOT),
        ("M80_FIRECRACKER_BIN", "/opt/firecracker/bin/firecracker"),
        ("M80_JAILER_BIN", "/opt/firecracker/bin/jailer"),
        (
            "M80_FIRECRACKER_SECCOMP_FILTER",
            "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
        ),
        ("M80_JAILER_HARDEN_BIN", "/opt/m80/bin/m80-jailer-harden"),
        ("M80_NET_HELPER_BIN", "/opt/m80/bin/m80-net-helper"),
        ("M80_KERNEL_KIND", "stripped"),
    ]:
        require_command_env_value(check, label, command, env, expected)
    for env in ["M80_JAIL_UID", "M80_JAIL_GID"]:
        check.require(f"{env}=" in command, f"{label}: command must set {env}")
    if kernel_image is None:
        require_command_env_absolute_path(check, label, command, "M80_KERNEL_IMAGE")
    else:
        require_command_env_value(check, label, command, "M80_KERNEL_IMAGE", kernel_image)
    if rootfs_image is None:
        require_command_env_absolute_path(check, label, command, "M80_ROOTFS_IMAGE")
    else:
        require_command_env_value(check, label, command, "M80_ROOTFS_IMAGE", rootfs_image)


def verify_composed_doc_consistency(
    doc_path: Path,
    restore_path: Path,
    memory_path: Path,
    residue_path: Path,
) -> list[str]:
    text = load_text(doc_path)
    restore = load_json(restore_path)
    memory = load_json(memory_path)
    residue = load_json(residue_path)
    check = Check()
    method = markdown_section(text, "Method")
    method_text = method or ""
    command = markdown_shell_block(method) if method is not None else None
    restore_section = markdown_section(text, "Restore latency")
    host_memory_section = markdown_section(text, "Host memory delta")
    residue_section = markdown_section(text, "Residue")

    commit = restore.get("git_commit")
    if isinstance(commit, str):
        check.require(
            commit in method_text,
            "composed doc: Method section missing measured git_commit from JSON artifacts",
        )
    run_id = restore.get("run_id")
    if isinstance(run_id, str):
        check.require(
            run_id in method_text,
            "composed doc: Method section missing run_id from JSON artifacts",
        )

    preflight = at(restore, "substrate.preflight_artifacts")
    if isinstance(preflight, dict):
        for field in [
            "firecracker_bin",
            "firecracker_seccomp_filter",
            "jailer_bin",
            "jailer_harden_bin",
            "net_helper_bin",
            "kernel_image",
            "rootfs_image",
            "expected_firecracker_version",
            "kernel_image_sha256",
            "rootfs_image_sha256",
        ]:
            value = preflight.get(field)
            check.require(
                isinstance(value, str) and value in method_text,
                f"composed doc: Method section missing "
                f"substrate.preflight_artifacts.{field} from JSON artifacts",
            )
        if command is not None:
            for env, field in [
                ("M80_FIRECRACKER_BIN", "firecracker_bin"),
                ("M80_JAILER_BIN", "jailer_bin"),
                ("M80_FIRECRACKER_SECCOMP_FILTER", "firecracker_seccomp_filter"),
                ("M80_JAILER_HARDEN_BIN", "jailer_harden_bin"),
                ("M80_NET_HELPER_BIN", "net_helper_bin"),
                ("M80_KERNEL_IMAGE", "kernel_image"),
                ("M80_ROOTFS_IMAGE", "rootfs_image"),
            ]:
                value = preflight.get(field)
                if isinstance(value, str) and value:
                    require_command_env_value(check, "composed doc command", command, env, value)

    substrate_fields = [
        ("host_kernel_release", "host kernel release"),
        ("firecracker_version", "Firecracker version"),
        ("dev_kvm_stat", "/dev/kvm stat"),
        ("sudo_uid", "sudo uid"),
    ]
    for field, label in substrate_fields:
        value = at(restore, f"substrate.{field}")
        if isinstance(value, str):
            check.require(
                f"{label}: `{value}`" in method_text,
                f"composed doc: Method section missing substrate.{field} from JSON artifact",
            )

    shared_digest = at(memory, "data.host_memory.shared_image_digest")
    residue_shared = at(residue, "data.residue.image_store.shared_digest")
    if isinstance(shared_digest, str) and shared_digest == residue_shared:
        check.require(
            shared_digest in (host_memory_section or ""),
            "composed doc: missing Shared image digest from host-memory section",
        )
        check.require(
            shared_digest in (residue_section or ""),
            "composed doc: missing Shared image digest from residue section",
        )
    for field in ["p50_ms", "p95_ms", "p99_ms"]:
        value = at(restore, f"data.restore_latency.{field}")
        if is_number(value):
            require_text_contains_number(
                check,
                f"composed doc: restore {field}",
                restore_section or "",
                value,
                f"{float(value):.3f} ms",
            )
    for field in [
        "after_n_attached_delta_bytes",
        "shared_image_bytes",
        "per_vm_overhead_bytes",
        "bound_bytes",
    ]:
        value = at(memory, f"data.host_memory.{field}")
        if is_number(value):
            require_text_contains_number(
                check,
                f"composed doc: host_memory {field}",
                host_memory_section or "",
                value,
                f"{int(value):,}",
            )
    scanned_roots = at(residue, "data.residue.scanned_roots")
    if isinstance(scanned_roots, list):
        if command is not None and scanned_roots and isinstance(scanned_roots[0], str):
            require_command_env_value(
                check,
                "composed doc command",
                command,
                "M80_RUN_ROOT",
                scanned_roots[0],
            )
        for root in [root for root in scanned_roots if isinstance(root, str)]:
            display_root = root
            if "/composed-e2e-templates-" in root and root.endswith("/templates"):
                display_root = re.sub(
                    r"composed-e2e-templates-[^/]+",
                    "composed-e2e-templates-*",
                    root,
                )
            check.require(
                f"`{root}`" in (residue_section or "")
                or f"`{display_root}`" in (residue_section or ""),
                f"composed doc: missing residue scanned root from JSON artifacts: {display_root}",
            )
    return check.errors


def verify_composed_doc(path: Path) -> list[str]:
    text = load_text(path)
    check = Check()
    method = markdown_section(text, "Method")
    command = markdown_shell_block(method) if method is not None else None
    check.require(text.startswith("# Composed E2E"), "composed doc: title mismatch")
    for heading in [
        "## Method",
        "## Restore latency",
        "## Host memory delta",
        "## Residue",
        "## Smoke evidence",
    ]:
        check.require(heading in text, f"composed doc: missing {heading}")
    for artifact in [
        "composed-e2e-restore-N10.json",
        "composed-e2e-host-memory.json",
        "composed-e2e-residue.json",
    ]:
        check.require(artifact in text, f"composed doc: missing {artifact}")
    smoke = markdown_section(text, "Smoke evidence")
    check.require(smoke is not None, "composed doc: missing Smoke evidence section")
    smoke_text = smoke or ""
    smoke_artifact_paths = re.findall(
        r"^M80_COMPOSED_E2E_ARTIFACT\s+(\S+)$",
        smoke_text,
        re.MULTILINE,
    )
    check.require(
        len(smoke_artifact_paths) == 3,
        "composed doc: smoke evidence must list exactly three artifact writes",
    )
    for suffix in [
        "crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json",
        "crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json",
        "crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json",
    ]:
        check.require(
            any(path.endswith(suffix) for path in smoke_artifact_paths),
            f"composed doc: smoke evidence missing artifact write {suffix}",
        )
    check.require(
        "test composed_e2e_layered_warm_pool ... ok" in smoke_text,
        "composed doc: smoke evidence must show composed_e2e_layered_warm_pool passed",
    )
    check.require(
        "test result: ok." in smoke_text,
        "composed doc: smoke evidence must show green test result",
    )
    for required in [
        "cargo test --release -p m80-firecracker --test e2e_composed_real_kvm",
        "M80_COMPOSED_E2E_ALLOW_OTHER_VMS=0",
        "M80_COMPOSED_E2E_N=10",
        "M80_COMPOSED_E2E_SHARED_PAYLOAD_MIB=32",
        "M80_COMPOSED_E2E_OUT_DIR=/tank/projects/m80/crates/m80-firecracker/benches/snapshots",
        "M80_RUN_ROOT=/var/lib/m80-composed-e2e",
        "M80_JAIL_UID=",
        "M80_JAIL_GID=",
        "M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker",
        "M80_JAILER_BIN=/opt/firecracker/bin/jailer",
        "M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin",
        "M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden",
        "M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper",
        "M80_ROOTFS_IMAGE=",
        "M80_KERNEL_IMAGE=",
        "M80_KERNEL_KIND=stripped",
        "--ignored composed_e2e_layered_warm_pool --nocapture",
        "Host:",
        "Firecracker",
        "real `/dev/kvm`",
        "real jailer",
        "Page cache was not dropped inside the run.",
        "P50",
        "P95",
        "P99",
        "<= 200 ms",
        "after_n_attached_delta_bytes",
        "Scanned roots:",
    ]:
        check.require(required in text, f"composed doc: missing {required}")
    check.require(command is not None, "composed doc: missing Method command shell block")
    if command is not None:
        verify_composed_command(check, "composed doc command", command)
    for marker in [
        "Status: scaffold and noisy-host diagnostic only",
        "verified-close receipt for `m80-q420k.6.5`; the close-quality rerun",
        "The host was not fully quiet",
        "not by itself a strict quiet-host verified close",
        "unrelated `t2-warm-slot-*` Firecracker processes were present",
    ]:
        check.require(marker not in text, f"composed doc: diagnostic marker remains: {marker}")
    return check.errors


def verify_composed_instruction_doc(path: Path) -> list[str]:
    text = load_text(path)
    check = Check()
    section = markdown_section(text, "E13. Composed E2E")
    section_text = section or ""
    command = markdown_shell_block(section) if section is not None else None
    check.require(path.is_file(), f"composed instruction doc: missing {path}")
    check.require(section is not None, "composed instruction doc: missing E13. Composed E2E section")
    for required in [
        "cargo test --release -p m80-firecracker --test e2e_composed_real_kvm",
        "M80_COMPOSED_E2E_ALLOW_OTHER_VMS=0",
        "M80_COMPOSED_E2E_N=10",
        "M80_COMPOSED_E2E_SHARED_PAYLOAD_MIB=32",
        "M80_COMPOSED_E2E_OUT_DIR=/tank/projects/m80/crates/m80-firecracker/benches/snapshots",
        "M80_RUN_ROOT=/var/lib/m80-composed-e2e",
        "M80_JAIL_UID=",
        "M80_JAIL_GID=",
        "M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker",
        "M80_JAILER_BIN=/opt/firecracker/bin/jailer",
        "M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin",
        "M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden",
        "M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper",
        f"M80_ROOTFS_IMAGE={PREPARED_ROOTFS_IMAGE}",
        f"M80_KERNEL_IMAGE={PREPARED_STRIPPED_KERNEL_IMAGE}",
        "M80_KERNEL_KIND=stripped",
        "--ignored composed_e2e_layered_warm_pool --nocapture",
        "crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json",
        "crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json",
        "crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json",
    ]:
        check.require(required in section_text, f"composed instruction doc: missing {required}")
    check.require(command is not None, "composed instruction doc: missing E13 command shell block")
    if command is not None:
        verify_composed_command(
            check,
            "composed instruction command",
            command,
            kernel_image=PREPARED_STRIPPED_KERNEL_IMAGE,
            rootfs_image=PREPARED_ROOTFS_IMAGE,
        )
    return check.errors


def verify_ext4_overlay(path: Path) -> list[str]:
    text = load_text(path)
    check = Check()
    command = markdown_shell_block(text)
    check.require(
        text.startswith("# Ext4 Overlay-Template Clone Measurement"),
        "ext4 overlay: title mismatch",
    )
    check.require(
        command is not None,
        "ext4 overlay: missing reproduction command block",
    )
    check.require(
        markdown_text(text, r"- git worktree dirty excluding this artifact: `([^`]+)`") == "false",
        "ext4 overlay: measured source worktree must be clean except for the artifact",
    )
    check.require(
        "## Device-Mapper Comparison" in text,
        "ext4 overlay: missing Device-Mapper Comparison section",
    )
    check.require(
        "dm-snapshot was not prototyped" in text,
        "ext4 overlay: missing dm-snapshot non-prototype explanation",
    )

    fstype = markdown_text(text, r"- host filesystem: `([^`]+)` mounted")
    kernel = markdown_text(text, r"- kernel: `([^`]+)`")
    run_root = markdown_text(text, r"- run root: `([^`]+)`")
    artifact = markdown_text(text, r"- artifact: `([^`]+)`")
    commit = markdown_text(text, r"- commit: `([^`]+)`")
    substrate_kind = markdown_text(text, r"- substrate kind: `([^`]+)`")
    overlay_size = markdown_int(text, r"- overlay size bytes: `(\d+)`")
    template_size = markdown_int(text, r"- template logical size bytes: `(\d+)`")
    template_allocated = markdown_int(text, r"- template allocated bytes after hole digging: `(\d+)`")
    clone_mode = markdown_text(text, r"- clone mode: `([^`]+)`")
    samples = markdown_int(text, r"- samples: `(\d+)`")
    p50 = markdown_float(text, r"- phase_3b_rootfs_prepare p50_ms: `([^`]+)`")
    p95 = markdown_float(text, r"- phase_3b_rootfs_prepare p95_ms: `([^`]+)`")
    p99 = markdown_float(text, r"- phase_3b_rootfs_prepare p99_ms: `([^`]+)`")
    min_ms = markdown_float(text, r"- phase_3b_rootfs_prepare min_ms: `([^`]+)`")
    max_ms = markdown_float(text, r"- phase_3b_rootfs_prepare max_ms: `([^`]+)`")
    leaked_dm = markdown_int(text, r"- leaked dm devices: `(\d+)`")
    mounts_before = markdown_int(text, r"- mount entries under run root before: `(\d+)`")
    mounts_after = markdown_int(text, r"- mount entries under run root after: `(\d+)`")
    leaked_mounts = markdown_int(text, r"- leaked mounts: `(\d+)`")
    run_root_removed = markdown_text(text, r"- run root removed: `([^`]+)`")
    threshold_result = markdown_text(text, r"- threshold result: `([^`]+)`")

    check.require(fstype == "ext4", "ext4 overlay: host filesystem must be ext4")
    check.require(isinstance(kernel, str) and bool(kernel), "ext4 overlay: kernel missing")
    check.require(
        isinstance(run_root, str) and run_root.startswith("/"),
        "ext4 overlay: run root must be an absolute path",
    )
    check.require(
        isinstance(artifact, str) and artifact.endswith("docs/perf/ext4-overlay-template-clone.md"),
        "ext4 overlay: artifact path must name docs/perf/ext4-overlay-template-clone.md",
    )
    check.require(
        isinstance(commit, str) and re.fullmatch(r"[0-9a-f]{40}", commit) is not None,
        "ext4 overlay: commit must be a full 40-character commit sha",
    )
    check.require(
        substrate_kind in {"storage-only", "real-kvm"},
        "ext4 overlay: substrate kind must be storage-only or real-kvm",
    )
    check.require(
        clone_mode == "byte-copy fallback",
        "ext4 overlay: clone mode must be byte-copy fallback",
    )
    check.require(is_number(overlay_size) and overlay_size > 0, "ext4 overlay: overlay size must be > 0")
    check.require(is_number(template_size) and template_size > 0, "ext4 overlay: template size must be > 0")
    check.require(
        is_number(template_allocated) and template_allocated > 0,
        "ext4 overlay: template allocated bytes must be > 0",
    )
    if is_number(template_size) and is_number(template_allocated):
        check.require(
            template_allocated <= template_size,
            "ext4 overlay: template allocated bytes must not exceed logical size",
        )
    require_number_at_least(check, "ext4 overlay: samples", samples, 30)
    check.require(is_number(p50), "ext4 overlay: p50_ms must be numeric")
    check.require(is_number(p95), "ext4 overlay: p95_ms must be numeric")
    check.require(is_number(p99), "ext4 overlay: p99_ms must be numeric")
    check.require(is_number(min_ms), "ext4 overlay: min_ms must be numeric")
    check.require(is_number(max_ms), "ext4 overlay: max_ms must be numeric")
    if all(is_number(value) for value in [min_ms, p50, p95, p99, max_ms]):
        check.require(
            min_ms <= p50 <= p95 <= p99 <= max_ms,
            "ext4 overlay: latency percentiles must be ordered min <= p50 <= p95 <= p99 <= max",
        )
    if is_number(p50):
        check.require(p50 <= 80.0, f"ext4 overlay: p50_ms {p50} exceeds 80")
    if is_number(p95):
        check.require(p95 <= 100.0, f"ext4 overlay: p95_ms {p95} exceeds 100")
    check.require(leaked_dm == 0, "ext4 overlay: leaked dm devices must be 0")
    check.require(is_number(mounts_before), "ext4 overlay: mount entries before must be numeric")
    check.require(is_number(mounts_after), "ext4 overlay: mount entries after must be numeric")
    check.require(leaked_mounts == 0, "ext4 overlay: leaked mounts must be 0")
    check.require(run_root_removed == "true", "ext4 overlay: run root removed must be true")
    check.require(
        threshold_result == "keep byte-copy fallback",
        "ext4 overlay: threshold result must be keep byte-copy fallback",
    )
    if command is not None:
        for token in [
            "M80_RUN_EXT4_OVERLAY_TEMPLATE_CLONE=1",
            f"M80_EXT4_OVERLAY_TEMPLATE_SAMPLES={samples}",
            "M80_EXT4_OVERLAY_TEMPLATE_RUN_ROOT=",
            "M80_EXT4_OVERLAY_TEMPLATE_ARTIFACT=",
            "cargo test -p m80-storage --test ext4_overlay_template_clone -- --ignored --nocapture",
        ]:
            check.require(token in command, f"ext4 overlay: reproduction command missing {token}")
        if isinstance(run_root, str):
            check.require(run_root in command, "ext4 overlay: reproduction command missing measured run root")
        if isinstance(artifact, str):
            check.require(artifact in command, "ext4 overlay: reproduction command missing measured artifact path")
    return check.errors


def verify_dax_memory_pressure(path: Path) -> list[str]:
    text = load_text(path)
    check = Check()
    check.require(
        text.startswith("# Pmem DAX Memory Pressure"),
        "dax memory pressure: title mismatch",
    )
    command = markdown_shell_block(text)
    check.require(command is not None, "dax memory pressure: missing reproduction command block")
    check.require(
        markdown_text(text, r"- git worktree dirty excluding this artifact: `([^`]+)`") == "false",
        "dax memory pressure: measured source worktree must be clean except for the artifact",
    )

    substrate_kind = markdown_text(text, r"- substrate kind: `([^`]+)`")
    commit = markdown_text(text, r"- commit: `([^`]+)`")
    vm_count = markdown_int(text, r"- VM count: `(\d+)`")
    samples = markdown_int(text, r"- samples per guest: `(\d+)`")
    payload_mib = markdown_int(text, r"- payload size: `(\d+) MiB`")
    fs = markdown_text(text, r"- host filesystem: `([^`]+)`")
    kernel = markdown_text(text, r"- host kernel: `([^`]+)`")
    memory = markdown_text(text, r"- host memory: `([^`]+)`")
    firecracker = markdown_text(text, r"- Firecracker version: `([^`]+)`")
    digest = markdown_text(text, r"- image digest: `([^`]+)`")
    image_path = markdown_text(text, r"- image path: `([^`]+)`")
    layout = markdown_int(text, r"- payload erofs layout: `Layout: (\d+)`,")
    payload_size = markdown_int(text, r"size `(\d+)` bytes, on-disk size")
    payload_on_disk = markdown_int(text, r"on-disk size `(\d+)` bytes")
    pressure_command = markdown_text(text, r"- pressure command: `([^`]+)`")
    unrelated_vms = markdown_text(text, r"- unrelated VMs running: `([^`]+)`")
    try:
        artifact_path = path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        artifact_path = path.as_posix()

    check.require(substrate_kind == "real-kvm", "dax memory pressure: substrate kind must be real-kvm")
    check.require(
        isinstance(commit, str) and re.fullmatch(r"[0-9a-f]{40}", commit) is not None,
        "dax memory pressure: commit must be a full 40-character commit sha",
    )
    require_number_at_least(check, "dax memory pressure: VM count", vm_count, 2)
    require_number_at_least(check, "dax memory pressure: samples per guest", samples, 3)
    require_number_at_least(check, "dax memory pressure: payload size MiB", payload_mib, 1)
    check.require(bool(kernel), "dax memory pressure: missing host kernel")
    check.require(bool(fs), "dax memory pressure: missing host filesystem")
    check.require(bool(memory), "dax memory pressure: missing host memory")
    check.require(bool(firecracker), "dax memory pressure: missing Firecracker version")
    require_sha256_hex(check, "dax memory pressure: image digest", digest)
    check.require(
        isinstance(image_path, str) and image_path.startswith("/"),
        "dax memory pressure: image path must be absolute",
    )
    if isinstance(image_path, str) and isinstance(digest, str):
        check.require(
            digest in image_path,
            "dax memory pressure: image path must contain image digest",
        )
    check.require(unrelated_vms == "false", "dax memory pressure: unrelated VMs running must be false")
    check.require(layout == 0, "dax memory pressure: payload erofs layout must be 0")
    require_number_at_least(check, "dax memory pressure: payload size", payload_size, 1)
    require_number_at_least(check, "dax memory pressure: payload on-disk size", payload_on_disk, 1)
    if is_number(payload_size) and is_number(payload_on_disk):
        check.require(
            payload_size == payload_on_disk,
            "dax memory pressure: payload logical and on-disk size must match",
        )
    if command is not None:
        for required in [
            "M80_RUN_PMEM_DAX_MEMORY_PRESSURE=1",
            "M80_PMEM_DAX_MEMORY_PRESSURE_COMMAND=",
            f"M80_PMEM_DAX_MEMORY_PRESSURE_VM_COUNT={vm_count}",
            f"M80_PMEM_DAX_MEMORY_PRESSURE_SAMPLES={samples}",
            f"M80_PMEM_DAX_MEMORY_PRESSURE_PAYLOAD_MIB={payload_mib}",
            f"M80_PMEM_DAX_MEMORY_PRESSURE_ARTIFACT={artifact_path}",
            "cargo test -p m80-firecracker --test pmem_dax_memory_pressure_real_kvm",
            "--ignored --nocapture",
        ]:
            check.require(required in command, f"dax memory pressure: reproduction command missing {required}")
        if isinstance(pressure_command, str):
            check.require(
                pressure_command in command,
                "dax memory pressure: reproduction command missing pressure command",
            )

    baseline_p50 = markdown_float(text, r"- baseline Shared-payload read latency p50_ms: `([^`]+)`")
    baseline_p95 = markdown_float(text, r"- baseline Shared-payload read latency p95_ms: `([^`]+)`")
    baseline_p99 = markdown_float(text, r"- baseline Shared-payload read latency p99_ms: `([^`]+)`")
    post_p50 = markdown_float(text, r"- post-pressure Shared-payload read latency p50_ms: `([^`]+)`")
    post_p95 = markdown_float(text, r"- post-pressure Shared-payload read latency p95_ms: `([^`]+)`")
    post_p99 = markdown_float(text, r"- post-pressure Shared-payload read latency p99_ms: `([^`]+)`")
    signal_delta = markdown_float(text, r"- cross-guest signal delta p50_ms: `([^`]+)`")
    for label, value in [
        ("baseline Shared-payload read latency p50_ms", baseline_p50),
        ("baseline Shared-payload read latency p95_ms", baseline_p95),
        ("baseline Shared-payload read latency p99_ms", baseline_p99),
        ("post-pressure Shared-payload read latency p50_ms", post_p50),
        ("post-pressure Shared-payload read latency p95_ms", post_p95),
        ("post-pressure Shared-payload read latency p99_ms", post_p99),
        ("cross-guest signal delta p50_ms", signal_delta),
        ("host memory delta during pressure bytes", markdown_float(text, r"- host memory delta during pressure bytes: `([^`]+)`")),
        ("host page-cache delta during pressure bytes", markdown_float(text, r"- host page-cache delta during pressure bytes: `([^`]+)`")),
        ("host memory delta after refault bytes", markdown_float(text, r"- host memory delta after refault bytes: `([^`]+)`")),
        ("host page-cache delta after refault bytes", markdown_float(text, r"- host page-cache delta after refault bytes: `([^`]+)`")),
    ]:
        check.require(
            is_number(value) and value >= 0,
            f"dax memory pressure: {label} must be numeric",
        )
    if all(is_number(value) for value in [baseline_p50, baseline_p95, baseline_p99]):
        check.require(
            baseline_p50 <= baseline_p95 <= baseline_p99,
            "dax memory pressure: baseline latency percentiles must be ordered p50 <= p95 <= p99",
        )
    if all(is_number(value) for value in [post_p50, post_p95, post_p99]):
        check.require(
            post_p50 <= post_p95 <= post_p99,
            "dax memory pressure: post-pressure latency percentiles must be ordered p50 <= p95 <= p99",
        )
    if is_number(baseline_p50) and is_number(post_p50) and is_number(signal_delta):
        expected_delta = max(0.0, post_p50 - baseline_p50)
        require_number_close(
            check,
            "dax memory pressure: cross-guest signal delta p50_ms",
            signal_delta,
            expected_delta,
            0.001,
        )

    substrate = markdown_json_block(text, "Firecracker process substrate")
    check.require(isinstance(substrate, dict), "dax memory pressure: missing Firecracker process substrate JSON")
    if isinstance(substrate, dict):
        substrate_data = {"substrate": substrate}
        quiet_substrate(substrate_data, "dax memory pressure", check)
        runtime_substrate(substrate_data, "dax memory pressure", check)
        preflight = substrate.get("preflight_artifacts")
        if command is not None and isinstance(preflight, dict):
            for env, field in [
                ("M80_FIRECRACKER_BIN", "firecracker_bin"),
                ("M80_JAILER_BIN", "jailer_bin"),
                ("M80_FIRECRACKER_SECCOMP_FILTER", "firecracker_seccomp_filter"),
                ("M80_JAILER_HARDEN_BIN", "jailer_harden_bin"),
                ("M80_NET_HELPER_BIN", "net_helper_bin"),
                ("M80_KERNEL_IMAGE", "kernel_image"),
                ("M80_KERNEL_KIND", "kernel_kind"),
                ("M80_ROOTFS_IMAGE", "rootfs_image"),
            ]:
                value = preflight.get(field)
                if isinstance(value, str):
                    require_command_env_value(check, "dax memory pressure", command, env, value)

    check.require(
        markdown_int(text, r"- leaked Shared markers: `(\d+)`") == 0,
        "dax memory pressure: leaked Shared markers must be 0",
    )
    check.require(
        markdown_int(text, r"- leaked Firecracker/jailer processes: `(\d+)`") == 0,
        "dax memory pressure: leaked Firecracker/jailer processes must be 0",
    )
    check.require(
        markdown_int(text, r"- leaked mounts: `(\d+)`") == 0,
        "dax memory pressure: leaked mounts must be 0",
    )
    check.require(
        "## Decision Output" in text,
        "dax memory pressure: missing Decision Output section",
    )
    decision = markdown_section(text, "Decision Output") or ""
    check.require(
        "same-trust-domain" in decision,
        "dax memory pressure: Decision Output must mention same-trust-domain interpretation",
    )
    return check.errors


def markdown_int(text: str, pattern: str) -> int | None:
    value = markdown_text(text, pattern)
    if value is None:
        return None
    try:
        return int(value)
    except ValueError:
        return None


def markdown_float(text: str, pattern: str) -> float | None:
    value = markdown_text(text, pattern)
    if value is None:
        return None
    try:
        return float(value)
    except ValueError:
        return None


def markdown_text(text: str, pattern: str) -> str | None:
    match = re.search(pattern, text)
    if match is None:
        return None
    return match.group(1)


def markdown_shell_block(text: str) -> str | None:
    match = re.search(r"```sh\n(?P<body>.*?)\n```", text, flags=re.DOTALL)
    if match is None:
        return None
    return match.group("body")


def markdown_json_block(text: str, heading: str) -> Any | None:
    matches = re.findall(
        rf"### {re.escape(heading)}\n\n```json\n(?P<body>.*?)\n```\n",
        text,
        flags=re.DOTALL,
    )
    if len(matches) != 1:
        return None
    try:
        return json.loads(matches[0])
    except json.JSONDecodeError:
        return None


def pmem_density_samples(text: str) -> list[dict[str, int]] | None:
    section = markdown_section(text, "Samples")
    if section is None:
        return None
    samples: list[dict[str, int]] = []
    for line in section.splitlines():
        match = re.fullmatch(
            r"\|\s*(?P<cycle>\d+)\s*"
            r"\|\s*(?P<before>\d+)\s*"
            r"\|\s*(?P<after>\d+)\s*"
            r"\|\s*(?P<delta>\d+)\s*"
            r"\|\s*(?P<bound>\d+)\s*\|",
            line,
        )
        if match is not None:
            samples.append(
                {
                    "cycle": int(match.group("cycle")),
                    "before_kib": int(match.group("before")),
                    "after_kib": int(match.group("after")),
                    "delta_kib": int(match.group("delta")),
                    "bound_kib": int(match.group("bound")),
                }
            )
    return samples


def markdown_section(text: str, heading: str) -> str | None:
    match = re.search(
        rf"^## {re.escape(heading)}\n\n(?P<body>.*?)(?=^## |\Z)",
        text,
        flags=re.DOTALL | re.MULTILINE,
    )
    if match is None:
        return None
    return match.group("body")


def is_number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def is_non_negative_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def kernel_version_at_least(value: Any, major: int, minor: int) -> bool:
    if not isinstance(value, str):
        return False
    match = re.match(r"(?P<major>\d+)\.(?P<minor>\d+)(?:\.|-|$)", value)
    if match is None:
        return False
    parsed = (int(match.group("major")), int(match.group("minor")))
    return parsed >= (major, minor)


def verify_committed_artifacts(check_specs: list[tuple[str, str, Any, Path]]) -> list[str]:
    errors: list[str] = []
    for key, label, _, path in check_specs:
        if key in {"close-artifact-paths", "prepared-inputs"}:
            continue
        try:
            rel_path = path.resolve().relative_to(ROOT)
        except ValueError:
            errors.append(f"{label}: path must be under repository root for --require-committed: {path}")
            continue
        rel = rel_path.as_posix()
        if git_ok(["cat-file", "-e", f"HEAD:{rel}"]) is False:
            errors.append(f"{label}: path is not committed in HEAD: {rel}")
        if git_ok(["diff", "--quiet", "--", rel]) is False:
            errors.append(f"{label}: path has unstaged changes: {rel}")
        if git_ok(["diff", "--cached", "--quiet", "--", rel]) is False:
            errors.append(f"{label}: path has staged changes: {rel}")
    return errors


def verify_close_artifact_paths(_: Path) -> list[str]:
    errors: list[str] = []
    for path in REQUIRED_CLOSE_ARTIFACT_PATHS:
        try:
            rel_path = path.resolve().relative_to(ROOT)
        except ValueError:
            errors.append(f"close artifact path: path must be under repository root: {path}")
            continue
        rel = rel_path.as_posix()
        if not path.parent.is_dir():
            errors.append(f"close artifact path: parent directory is missing: {path.parent}")
        if git_ok(["check-ignore", "-q", "--", rel]):
            errors.append(f"close artifact path: path is ignored by git: {rel}")
    return errors


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_stdout(argv: list[str]) -> str | None:
    try:
        result = subprocess.run(
            argv,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            check=False,
        )
    except OSError:
        return None
    if result.returncode != 0:
        return None
    return result.stdout.strip()


def require_prepared_input(
    check: Check,
    label: str,
    raw_path: str,
    expected_sha256: str,
    executable: bool,
) -> None:
    path = Path(raw_path)
    check.require(path.is_file(), f"prepared input {label}: missing file {raw_path}")
    if not path.is_file():
        return
    if executable:
        check.require(
            path.stat().st_mode & 0o111 != 0,
            f"prepared input {label}: file must be executable",
        )
    actual = sha256_file(path)
    check.require(
        actual == expected_sha256,
        f"prepared input {label}: sha256 {actual} != {expected_sha256}",
    )


def verify_prepared_inputs(_: Path) -> list[str]:
    check = Check()
    require_prepared_input(
        check,
        "stripped kernel",
        PREPARED_STRIPPED_KERNEL_IMAGE,
        PREPARED_STRIPPED_KERNEL_SHA256,
        executable=True,
    )
    require_prepared_input(
        check,
        "rootfs",
        PREPARED_ROOTFS_IMAGE,
        PREPARED_ROOTFS_SHA256,
        executable=False,
    )
    require_prepared_input(
        check,
        "Firecracker",
        "/opt/firecracker/bin/firecracker",
        PREPARED_FIRECRACKER_SHA256,
        executable=True,
    )
    require_prepared_input(
        check,
        "jailer",
        "/opt/firecracker/bin/jailer",
        PREPARED_JAILER_SHA256,
        executable=True,
    )
    require_prepared_input(
        check,
        "seccomp filter",
        "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
        PREPARED_SECCOMP_SHA256,
        executable=False,
    )
    require_prepared_input(
        check,
        "jailer harden helper",
        "/opt/m80/bin/m80-jailer-harden",
        PREPARED_JAILER_HARDEN_SHA256,
        executable=True,
    )
    require_prepared_input(
        check,
        "network helper",
        "/opt/m80/bin/m80-net-helper",
        PREPARED_NET_HELPER_SHA256,
        executable=True,
    )
    firecracker_version = run_stdout(["/opt/firecracker/bin/firecracker", "--version"])
    check.require(
        isinstance(firecracker_version, str) and "Firecracker v1.15.1" in firecracker_version,
        "prepared input Firecracker: --version must include Firecracker v1.15.1",
    )
    jailer_version = run_stdout(["/opt/firecracker/bin/jailer", "--version"])
    check.require(
        isinstance(jailer_version, str) and "Jailer v1.15.1" in jailer_version,
        "prepared input jailer: --version must include Jailer v1.15.1",
    )
    return check.errors


def verify_closed_beads(check_specs: list[tuple[str, str, Any, Path]]) -> list[str]:
    errors: list[str] = []
    for key, label, _, path in check_specs:
        bead_id = REQUIRED_CLOSE_BEADS.get(key)
        if bead_id is None:
            continue
        try:
            rel_path = path.resolve().relative_to(ROOT).as_posix()
        except ValueError:
            errors.append(f"{label}: path must be under repository root for close-reason check: {path}")
            continue
        issue = load_bead(bead_id, label, errors)
        if issue is None:
            continue
        status = issue.get("status")
        if status != "closed":
            errors.append(f"{label}: {bead_id} must be closed, got status={status!r}")
            continue
        reason = issue.get("close_reason")
        if not isinstance(reason, str):
            errors.append(f"{label}: {bead_id} missing close_reason")
            continue
        commit = close_reason_commit(reason, rel_path)
        if commit is None:
            errors.append(
                f"{label}: {bead_id} close_reason must contain "
                f"`verified: {rel_path} @ <commit-sha>`"
            )
            continue
        if git_ok(["cat-file", "-e", f"{commit}^{{commit}}"]) is False:
            errors.append(f"{label}: {bead_id} close_reason commit does not exist: {commit}")
            continue
        if git_ok(["merge-base", "--is-ancestor", commit, "HEAD"]) is False:
            errors.append(f"{label}: {bead_id} close_reason commit is not an ancestor of HEAD: {commit}")
        if git_ok(["cat-file", "-e", f"{commit}:{rel_path}"]) is False:
            errors.append(
                f"{label}: {bead_id} close_reason commit {commit} does not contain artifact {rel_path}"
            )
            continue
        if path.is_file():
            measured_commit = artifact_measured_git_commit(key, path)
            if key != "composed-doc" and measured_commit is None:
                errors.append(f"{label}: artifact must record git_commit for close-reason verification")
            elif measured_commit is not None:
                if git_ok(["cat-file", "-e", f"{measured_commit}^{{commit}}"]) is False:
                    errors.append(f"{label}: artifact git_commit does not exist: {measured_commit}")
                elif git_ok(["merge-base", "--is-ancestor", measured_commit, commit]) is False:
                    errors.append(
                        f"{label}: close_reason commit {commit} is not descended from artifact git_commit "
                        f"{measured_commit}"
                    )
        if key == "composed-doc":
            errors.extend(verify_composed_doc_close_reason(reason))
        if git_ok(["diff", "--quiet", commit, "HEAD", "--", rel_path]) is False:
            errors.append(
                f"{label}: {bead_id} artifact {rel_path} changed after close_reason commit {commit}"
            )
    return errors


def verify_parent_phases_closed() -> list[str]:
    errors: list[str] = []
    for bead_id in REQUIRED_PARENT_PHASES:
        issue = load_bead(bead_id, bead_id, errors)
        if issue is None:
            continue
        status = issue.get("status")
        if status != "closed":
            errors.append(f"parent phase: {bead_id} must be closed, got status={status!r}")
            continue
        close_reason = issue.get("close_reason")
        if not isinstance(close_reason, str):
            errors.append(f"parent phase: {bead_id} missing close_reason")
            continue
        errors.extend(verify_parent_phase_close_reason(bead_id, close_reason))
    return errors


def verify_parent_phase_close_reason(parent_id: str, parent_reason: str) -> list[str]:
    errors: list[str] = []
    for child_id, rel_path in PARENT_PHASE_CLOSE_REFERENCES.get(parent_id, []):
        child = load_bead(child_id, f"parent phase {parent_id}", errors)
        if child is None:
            continue
        status = child.get("status")
        if status != "closed":
            errors.append(f"parent phase: {parent_id} referenced child {child_id} must be closed, got {status!r}")
            continue
        child_reason = child.get("close_reason")
        if not isinstance(child_reason, str):
            errors.append(f"parent phase: {parent_id} referenced child {child_id} missing close_reason")
            continue
        child_commit = close_reason_commit(child_reason, rel_path)
        if child_commit is None:
            errors.append(
                f"parent phase: {child_id} close_reason must contain "
                f"`verified: {rel_path} @ <commit-sha>`"
            )
            continue
        parent_commit = close_reason_commit(parent_reason, rel_path)
        if parent_commit is None:
            errors.append(
                f"parent phase: {parent_id} close_reason must reference "
                f"`verified: {rel_path} @ <commit-sha>`"
            )
        elif not same_commit(parent_commit, child_commit):
            errors.append(
                f"parent phase: {parent_id} close_reason commit for {rel_path} must match "
                f"{child_id} ({child_commit}), got {parent_commit}"
            )
    return errors


def verify_tracker_graph_clear() -> list[str]:
    errors: list[str] = []
    blocked = br_json(["blocked", "--json"], "br blocked", errors)
    if isinstance(blocked, list):
        remaining = []
        for issue in blocked:
            if not isinstance(issue, dict) or issue.get("id") not in REQUIRED_UNBLOCKED_ISSUES:
                continue
            remaining.append(f"{issue.get('id')} blocked_by={issue.get('blocked_by')}")
        if remaining:
            errors.append(f"tracker graph: q420k blockers remain: {', '.join(remaining)}")
    cycles = br_json(["dep", "cycles", "--json"], "br dep cycles", errors)
    if isinstance(cycles, dict) and cycles.get("count") != 0:
        errors.append("tracker graph: dependency cycles remain")
    return errors


def verify_super_epic_closed() -> list[str]:
    errors: list[str] = []
    issue = load_bead("m80-q420k", "super epic", errors)
    if issue is None:
        return errors
    status = issue.get("status")
    if status != "closed":
        errors.append(f"super epic: m80-q420k must be closed, got status={status!r}")
        return errors
    reason = issue.get("close_reason")
    if not isinstance(reason, str):
        errors.append("super epic: m80-q420k missing close_reason")
        return errors
    errors.extend(verify_super_epic_close_reason(reason))
    return errors


def verify_super_epic_close_reason(reason: str) -> list[str]:
    errors: list[str] = []
    phase_id, rel_path = SUPER_EPIC_CLOSE_REFERENCE
    phase = load_bead(phase_id, "super epic", errors)
    if phase is None:
        return errors
    status = phase.get("status")
    if status != "closed":
        errors.append(f"super epic: referenced phase {phase_id} must be closed, got {status!r}")
        return errors
    phase_reason = phase.get("close_reason")
    if not isinstance(phase_reason, str):
        errors.append(f"super epic: referenced phase {phase_id} missing close_reason")
        return errors
    phase_commit = close_reason_commit(phase_reason, rel_path)
    if phase_commit is None:
        errors.append(
            f"super epic: referenced phase {phase_id} close_reason must contain "
            f"`verified: {rel_path} @ <commit-sha>`"
        )
        return errors
    return verify_super_epic_close_reason_ref(reason, phase_id, rel_path, phase_commit)


def verify_super_epic_close_reason_ref(
    reason: str,
    phase_id: str,
    rel_path: str,
    phase_commit: str,
) -> list[str]:
    errors: list[str] = []
    epic_commit = close_reason_commit(reason, rel_path)
    if epic_commit is None:
        errors.append(
            f"super epic: m80-q420k close_reason must reference `verified: {rel_path} @ <commit-sha>`"
        )
    elif not same_commit(epic_commit, phase_commit):
        errors.append(
            f"super epic: m80-q420k close_reason commit for {rel_path} must match "
            f"{phase_id} ({phase_commit}), got {epic_commit}"
        )
    return errors


def load_bead(bead_id: str, label: str, errors: list[str]) -> dict[str, Any] | None:
    data = br_json(["show", bead_id, "--json"], f"br show {bead_id}", errors)
    if data is None:
        return None
    if not isinstance(data, list) or len(data) != 1 or not isinstance(data[0], dict):
        errors.append(f"{label}: br show {bead_id} returned unexpected JSON shape")
        return None
    return data[0]


def br_json(args: list[str], label: str, errors: list[str]) -> Any | None:
    result = subprocess.run(
        ["br", *args],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        errors.append(f"{label} failed: {result.stderr.strip()}")
        return None
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        errors.append(f"{label} did not return JSON: {exc}")
        return None


def close_reason_matches(reason: str, rel_path: str) -> bool:
    return close_reason_commit(reason, rel_path) is not None


def verify_composed_doc_close_reason(reason: str) -> list[str]:
    errors: list[str] = []
    expected_refs: list[tuple[str, str, str]] = []
    for bead_id, rel_path in COMPOSED_MEASUREMENT_CLOSE_BEADS:
        issue = load_bead(bead_id, "composed doc", errors)
        if issue is None:
            continue
        status = issue.get("status")
        if status != "closed":
            errors.append(f"composed doc: referenced measurement bead {bead_id} must be closed, got {status!r}")
            continue
        measurement_reason = issue.get("close_reason")
        if not isinstance(measurement_reason, str):
            errors.append(f"composed doc: referenced measurement bead {bead_id} missing close_reason")
            continue
        measurement_commit = close_reason_commit(measurement_reason, rel_path)
        if measurement_commit is None:
            errors.append(
                f"composed doc: referenced measurement bead {bead_id} close_reason must contain "
                f"`verified: {rel_path} @ <commit-sha>`"
            )
            continue
        expected_refs.append((bead_id, rel_path, measurement_commit))
    errors.extend(verify_composed_doc_close_reason_refs(reason, expected_refs))
    return errors


def verify_composed_doc_close_reason_refs(
    reason: str,
    expected_refs: list[tuple[str, str, str]],
) -> list[str]:
    errors: list[str] = []
    for bead_id, rel_path, measurement_commit in expected_refs:
        doc_commit = close_reason_commit(reason, rel_path)
        if doc_commit is None:
            errors.append(
                "composed doc: m80-q420k.6.5 close_reason must reference "
                f"`verified: {rel_path} @ <commit-sha>`"
            )
        elif not same_commit(doc_commit, measurement_commit):
            errors.append(
                f"composed doc: m80-q420k.6.5 close_reason commit for {rel_path} "
                f"must match {bead_id} ({measurement_commit}), got {doc_commit}"
            )
    return errors


def artifact_measured_git_commit(key: str, path: Path) -> str | None:
    if key == "pmem-density":
        try:
            substrate_section = markdown_section(path.read_text(), "Substrate") or ""
        except FileNotFoundError:
            return None
        commit = markdown_text(substrate_section, r"- git commit: `([^`]+)`")
    elif key in {
        "snapshot-template",
        "composed-restore",
        "composed-memory",
        "composed-residue",
    }:
        try:
            data = json.loads(path.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            return None
        commit = data.get("git_commit") if isinstance(data, dict) else None
    else:
        return None
    if isinstance(commit, str) and re.fullmatch(r"[0-9a-f]{40}", commit) is not None:
        return commit
    return None


def close_reason_commit(reason: str, rel_path: str) -> str | None:
    pattern = rf"verified:\s+{re.escape(rel_path)}\s+@\s+[0-9a-f]{{7,40}}\b"
    match = re.search(pattern, reason)
    if match is None:
        return None
    return match.group(0).rsplit("@", maxsplit=1)[1].strip()


def same_commit(left: str, right: str) -> bool:
    left_full = git_stdout(["rev-parse", "--verify", f"{left}^{{commit}}"])
    right_full = git_stdout(["rev-parse", "--verify", f"{right}^{{commit}}"])
    return left_full is not None and right_full is not None and left_full == right_full


def git_ok(args: list[str]) -> bool:
    result = subprocess.run(
        ["git", "-C", str(ROOT), *args],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    return result.returncode == 0


def git_stdout(args: list[str]) -> str | None:
    result = subprocess.run(
        ["git", "-C", str(ROOT), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        return None
    return result.stdout.strip()


def run_checks(args: argparse.Namespace) -> int:
    check_specs = [
        (
            "close-artifact-paths",
            "close artifact path trackability",
            verify_close_artifact_paths,
            ROOT,
        ),
        (
            "quiet-host-inventory",
            "quiet-host inventory helper",
            verify_quiet_host_inventory,
            artifact_path(args.quiet_host_inventory),
        ),
        ("snapshot-template", "snapshot template", verify_snapshot_template, artifact_path(args.snapshot_template)),
        ("snapshot-doc", "snapshot doc", verify_snapshot_doc, artifact_path(args.snapshot_doc)),
        ("pmem-density", "pmem density", verify_pmem_density, artifact_path(args.pmem_density)),
        (
            "pmem-density-smoke",
            "pmem density smoke script",
            verify_pmem_density_smoke,
            artifact_path(args.pmem_density_smoke),
        ),
        (
            "pmem-density-runbook",
            "pmem density runbook instructions",
            verify_pmem_density_instruction_doc,
            artifact_path(args.close_runbook),
        ),
        (
            "pmem-density-playbook",
            "pmem density playbook instructions",
            verify_pmem_density_instruction_doc,
            artifact_path(args.measurement_playbook),
        ),
        ("composed-restore", "composed restore", verify_composed_restore, artifact_path(args.composed_restore)),
        ("composed-memory", "composed memory", verify_composed_memory, artifact_path(args.composed_memory)),
        ("composed-residue", "composed residue", verify_composed_residue, artifact_path(args.composed_residue)),
        ("composed-doc", "composed doc", verify_composed_doc, artifact_path(args.composed_doc)),
        (
            "composed-playbook",
            "composed playbook instructions",
            verify_composed_instruction_doc,
            artifact_path(args.measurement_playbook),
        ),
    ]
    optional_check_specs = [
        ("prepared-inputs", "prepared host inputs", verify_prepared_inputs, ROOT),
        ("ext4-overlay", "ext4 overlay", verify_ext4_overlay, artifact_path(args.ext4_overlay)),
        (
            "dax-memory-pressure",
            "dax memory pressure",
            verify_dax_memory_pressure,
            artifact_path(args.dax_memory_pressure),
        ),
    ]
    selected = set(getattr(args, "only", None) or [])
    if "snapshot-template" in selected:
        selected.add("snapshot-doc")
    if "pmem-density" in selected:
        selected.add("pmem-density-smoke")
        selected.add("pmem-density-runbook")
        selected.add("pmem-density-playbook")
    if "composed-doc" in selected:
        selected.update(["composed-restore", "composed-memory", "composed-residue"])
        selected.add("composed-playbook")
    if selected:
        check_specs = [spec for spec in [*check_specs, *optional_check_specs] if spec[0] in selected]
    checks: list[tuple[str, list[str]]] = []
    for _, label, fn, path in check_specs:
        try:
            checks.append((label, fn(path)))
        except AssertionError as exc:
            checks.append((label, [f"{label}: {exc}"]))
    errors = [error for _, group in checks for error in group]
    errors.extend(verify_flag_composition(args, selected))
    selected_keys = {key for key, _, _, _ in check_specs}
    if {"snapshot-template", "snapshot-doc"}.issubset(selected_keys):
        snapshot_path = artifact_path(args.snapshot_template)
        doc_path = artifact_path(args.snapshot_doc)
        if snapshot_path.is_file() and doc_path.is_file():
            try:
                errors.extend(verify_snapshot_doc_consistency(doc_path, snapshot_path))
            except AssertionError as exc:
                errors.append(f"snapshot doc consistency: {exc}")
    if {"composed-restore", "composed-memory", "composed-residue"}.issubset(selected_keys):
        composed_paths = [
            artifact_path(args.composed_restore),
            artifact_path(args.composed_memory),
            artifact_path(args.composed_residue),
        ]
        try:
            if all(path.is_file() for path in composed_paths):
                errors.extend(verify_composed_consistency(*composed_paths))
        except AssertionError as exc:
            errors.append(f"composed consistency: {exc}")
        doc_path = artifact_path(args.composed_doc)
        if "composed-doc" in selected_keys and doc_path.is_file() and all(path.is_file() for path in composed_paths):
            try:
                errors.extend(verify_composed_doc_consistency(doc_path, *composed_paths))
            except AssertionError as exc:
                errors.append(f"composed doc consistency: {exc}")
    if getattr(args, "require_committed", False):
        errors.extend(verify_committed_artifacts(check_specs))
    if getattr(args, "require_closed_beads", False):
        errors.extend(verify_closed_beads(check_specs))
    if getattr(args, "require_parent_phases_closed", False):
        errors.extend(verify_parent_phases_closed())
        errors.extend(verify_tracker_graph_clear())
    if getattr(args, "require_super_epic_closed", False):
        errors.extend(verify_super_epic_closed())
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    for label, _ in checks:
        print(f"ok: {label}")
    return 0


def verify_flag_composition(args: argparse.Namespace, selected: set[str]) -> list[str]:
    errors: list[str] = []
    require_committed = getattr(args, "require_committed", False)
    require_closed_beads = getattr(args, "require_closed_beads", False)
    require_parent = getattr(args, "require_parent_phases_closed", False)
    require_super = getattr(args, "require_super_epic_closed", False)
    if selected and require_parent:
        errors.append(
            "--require-parent-phases-closed cannot be combined with --only; "
            "run the full A-F guard for parent close"
        )
    if selected and require_super:
        errors.append(
            "--require-super-epic-closed cannot be combined with --only; "
            "run the full A-F guard for super-epic close"
        )
    if require_parent and not require_committed:
        errors.append("--require-parent-phases-closed requires --require-committed")
    if require_parent and not require_closed_beads:
        errors.append("--require-parent-phases-closed requires --require-closed-beads")
    if require_super and not require_parent:
        errors.append("--require-super-epic-closed requires --require-parent-phases-closed")
    return errors


def run_self_tests() -> int:
    global REQUIRED_CLOSE_ARTIFACT_PATHS
    with tempfile.TemporaryDirectory() as tmp_raw:
        tmp = Path(tmp_raw)
        snapshot = tmp / "snapshot.json"
        snapshot_doc = tmp / "snapshot-template-restore.md"
        density = tmp / "density.md"
        density_smoke = tmp / "smoke-pmem-shared.sh"
        close_runbook = tmp / "q420k-close-gates.md"
        measurement_playbook = tmp / "measurement-playbook.md"
        restore = tmp / "restore.json"
        memory = tmp / "memory.json"
        residue = tmp / "residue.json"
        composed_doc = tmp / "composed-e2e.md"
        ext4_overlay = tmp / "ext4-overlay.md"
        dax_pressure = tmp / "dax-pressure.md"
        quiet_host_inventory = tmp / "q420k-quiet-host-inventory.sh"
        preflight_artifacts = {
            "firecracker_bin": "/opt/firecracker/bin/firecracker",
            "firecracker_seccomp_filter": "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
            "jailer_bin": "/opt/firecracker/bin/jailer",
            "jailer_harden_bin": "/opt/m80/bin/m80-jailer-harden",
            "net_helper_bin": "/opt/m80/bin/m80-net-helper",
            "kernel_image": "/var/lib/m80/kernels/vmlinux",
            "rootfs_image": "/var/lib/m80/rootfs.ext4",
            "kernel_image_sha256": "a" * 64,
            "rootfs_image_sha256": "b" * 64,
            "kernel_kind": "stripped",
            "image_kind": "minimal",
            "rootfs_format": "ext4",
            "expected_firecracker_version": "v1.15.1",
        }
        git_commit = "c" * 40
        composed_run_id = "composed-1a2b"
        shared_digest = "1" * 64
        per_vm_digest = "2" * 64
        snapshot_samples_us = [199_000 for _ in range(SNAPSHOT_TEMPLATE_SAMPLES_TOTAL)]
        composed_run_root = "/var/lib/m80-composed-e2e"
        composed_template_root = f"{composed_run_root}/composed-e2e-templates-1/templates"
        substrate = {
            "substrate_kind": "real-kvm",
            "preflight_required": True,
            "quiet_host_checked": True,
            "allow_other_firecracker_vms": False,
            "preexisting_firecracker_processes": [],
            "post_run_firecracker_processes": [],
            "preflight_artifacts": preflight_artifacts,
            "host_kernel_release": "6.17.0-23-generic",
            "firecracker_version": "Firecracker v1.15.1",
            "dev_kvm_stat": "crw-rw---- root:kvm /dev/kvm",
            "sudo_uid": "0",
        }
        snapshot.write_text(json.dumps({
            "schema_version": 1,
            "bench": "snapshot_template_restore_latency",
            "load": "idle",
            "target_ready": 1,
            "n_per_run": SNAPSHOT_TEMPLATE_N_PER_RUN,
            "runs": SNAPSHOT_TEMPLATE_RUNS,
            "samples_total": SNAPSHOT_TEMPLATE_SAMPLES_TOTAL,
            "vcpu_count": 1,
            "mem_size_mib": 512,
            "jail_uid": 1000,
            "jail_gid": 1000,
            "cgroup_mode": "disabled",
            "page_cache_dropped_between_samples": True,
            "git_worktree_dirty_excluding_artifact": False,
            "git_commit": git_commit,
            "reproduction_command": (
                "M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS=0 "
                "M80_SNAPSHOT_BENCH_LOAD=idle "
                "N=20 "
                "M80_SNAPSHOT_TEMPLATE_RUNS=3 "
                "M80_SNAPSHOT_BENCH_VCPU_COUNT=1 "
                "M80_SNAPSHOT_BENCH_MEM_SIZE_MIB=512 "
                "M80_JAIL_UID=1000 "
                "M80_JAIL_GID=1000 "
                "M80_CGROUP_MODE=disabled "
                "M80_SNAPSHOT_TEMPLATE_BENCH_OUTPUT=crates/m80-firecracker/benches/snapshot_template_restore_latency.json "
                "M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker "
                "M80_JAILER_BIN=/opt/firecracker/bin/jailer "
                "M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin "
                "M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden "
                "M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper "
                "M80_KERNEL_IMAGE=/var/lib/m80/kernels/vmlinux "
                "M80_KERNEL_KIND=stripped "
                "M80_ROOTFS_IMAGE=/var/lib/m80/rootfs.ext4 "
                "M80_RUN_ROOT=/var/lib/m80/run "
                "cargo bench -p m80-firecracker --bench snapshot_template_restore_latency"
            ),
            "substrate": substrate,
            "data": {"warm": {
                "restore_to_handback_ms": {"p99": 199.0},
                "restore_to_handback_us": {"p99": 199_000},
                "samples_us": snapshot_samples_us,
                "sample_details": [
                    {
                        "run": index // SNAPSHOT_TEMPLATE_N_PER_RUN,
                        "cycle": index % SNAPSHOT_TEMPLATE_N_PER_RUN,
                        "fill_us": value - 1000,
                        "handoff_us": 1000,
                        "restore_to_handback_us": value,
                    }
                    for index, value in enumerate(snapshot_samples_us)
                ],
            }},
            "runs_detail": [
                {
                    "run": run,
                    "initial_fill_samples_us": [50_000],
                    "restore_to_handback_us": {"p99": 199_000},
                    "samples_us": [199_000 for _ in range(SNAPSHOT_TEMPLATE_N_PER_RUN)],
                }
                for run in range(SNAPSHOT_TEMPLATE_RUNS)
            ],
        }))
        snapshot_doc.write_text(
            """# Snapshot-Template Restore Latency

Artifact: `crates/m80-firecracker/benches/snapshot_template_restore_latency.json`
Field: `data.warm.restore_to_handback_ms.p99`
Observable: `target_ready=1`

## Artifact identity

Measured git commit:
`cccccccccccccccccccccccccccccccccccccccc`

Runtime substrate:

- host_kernel_release: `6.17.0-23-generic`
- firecracker_version: `Firecracker v1.15.1`
- dev_kvm_stat: `crw-rw---- root:kvm /dev/kvm`
- sudo_uid: `0`

Preflight artifacts:

- firecracker_bin: `/opt/firecracker/bin/firecracker`
- firecracker_seccomp_filter: `/opt/firecracker/bin/firecracker-seccomp-filter.bin`
- jailer_bin: `/opt/firecracker/bin/jailer`
- jailer_harden_bin: `/opt/m80/bin/m80-jailer-harden`
- net_helper_bin: `/opt/m80/bin/m80-net-helper`
- kernel_image: `/var/lib/m80/kernels/vmlinux`
- rootfs_image: `/var/lib/m80/rootfs.ext4`
- kernel_image_sha256: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`
- rootfs_image_sha256: `bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb`
- expected_firecracker_version: `v1.15.1`

## Command

```sh
sync && echo 3 | sudo tee /proc/sys/vm/drop_caches

sudo -n env \
  M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS=0 \
  M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
  M80_JAILER_BIN=/opt/firecracker/bin/jailer \
  M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin \
  M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden \
  M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper \
  M80_KERNEL_IMAGE=/var/lib/m80/kernels/vmlinux \
  M80_ROOTFS_IMAGE=/var/lib/m80/rootfs.ext4 \
  M80_SNAPSHOT_BENCH_LOAD=idle \
  M80_SNAPSHOT_BENCH_VCPU_COUNT=1 M80_SNAPSHOT_BENCH_MEM_SIZE_MIB=512 \
  M80_KERNEL_KIND=stripped \
  M80_RUN_ROOT=/var/lib/m80/run \
  M80_JAIL_UID=1000 M80_JAIL_GID=1000 M80_CGROUP_MODE=disabled \
  N=20 M80_SNAPSHOT_TEMPLATE_RUNS=3 \
  M80_SNAPSHOT_TEMPLATE_BENCH_OUTPUT=crates/m80-firecracker/benches/snapshot_template_restore_latency.json \
  cargo bench -p m80-firecracker --bench snapshot_template_restore_latency
```

Close reason:

```text
verified: crates/m80-firecracker/benches/snapshot_template_restore_latency.json @ <commit-sha>
```

Verify:

```sh
python3 scripts/verify-q420k-artifacts.py --only snapshot-template --require-committed
```

The verifier checks `git_commit`, `reproduction_command`,
`substrate.preflight_artifacts`, and `substrate.post_run_firecracker_processes`.

## Smoke evidence

```text
snapshot-template restore: load=idle runs=3 n=20 p99=199000us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json
```
"""
        )
        density.write_text(
            """# Shared pmem density

## Reproduction

Command: `M80_PMEM_SHARED_ALLOW_OTHER_VMS=0 M80_PMEM_SHARED_VM_COUNT=4 M80_PMEM_SHARED_CYCLES=10 M80_PMEM_SHARED_PAYLOAD_MIB=128 M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=1024 M80_PMEM_SHARED_DENSITY_ARTIFACT={density_path} M80_RUN_ROOT=/var/lib/m80-psd M80_KERNEL_IMAGE=/var/lib/m80/kernels/vmlinux M80_KERNEL_KIND=stripped M80_ROOTFS_IMAGE=/var/lib/m80/rootfs.ext4 M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker M80_JAILER_BIN=/opt/firecracker/bin/jailer M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper M80_FIRECRACKER_VERSION=v1.15.1 M80_JAIL_UID=1000 M80_JAIL_GID=1000 scripts/smoke-pmem-shared.sh`

## Substrate

- host kernel: `6.17.0-23-generic`
- firecracker: `Firecracker v1.15.1`
- `/dev/kvm`: `crw-rw---- root:kvm /dev/kvm`
- sudo: required; test ran as uid `0`
- dropped page cache before each cycle: `sync && echo 3 > /proc/sys/vm/drop_caches`
- git worktree dirty excluding this artifact: `false`
- git commit: `cccccccccccccccccccccccccccccccccccccccc`

### Firecracker process substrate

```json
{
  "substrate_kind": "real-kvm",
  "preflight_required": true,
  "quiet_host_checked": true,
  "allow_other_firecracker_vms": false,
  "preexisting_firecracker_processes": [],
  "post_run_firecracker_processes": [],
  "host_kernel_release": "6.17.0-23-generic",
  "firecracker_version": "Firecracker v1.15.1",
  "dev_kvm_stat": "crw-rw---- root:kvm /dev/kvm",
  "sudo_uid": "0",
  "preflight_artifacts": {
    "firecracker_bin": "/opt/firecracker/bin/firecracker",
    "firecracker_seccomp_filter": "/opt/firecracker/bin/firecracker-seccomp-filter.bin",
    "jailer_bin": "/opt/firecracker/bin/jailer",
    "jailer_harden_bin": "/opt/m80/bin/m80-jailer-harden",
    "net_helper_bin": "/opt/m80/bin/m80-net-helper",
    "kernel_image": "/var/lib/m80/kernels/vmlinux",
    "rootfs_image": "/var/lib/m80/rootfs.ext4",
    "kernel_image_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "rootfs_image_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "kernel_kind": "stripped",
    "image_kind": "minimal",
    "rootfs_format": "ext4",
    "expected_firecracker_version": "v1.15.1"
  }
}
```

## Observable

- field: host memory delta after 4 attached Shared VMs
- cycles: `10`
- payload size: `128 MiB`
- image digest: `1111111111111111111111111111111111111111111111111111111111111111`
- image path: `/var/lib/m80-images/11/1111111111111111111111111111111111111111111111111111111111111111/image.erofs`
- payload erofs layout: `Layout: 0`, size `4096` bytes, on-disk size `4096` bytes, compression ratio `0.00%`
- image KiB: `4096`
- per-VM overhead bound: `1024 KiB`
- bound: `8192 KiB`
- max observed delta: `4096 KiB`
- result: `pass`

## Teardown

- max active-use markers observed: `4`
- final active-use markers: `0`
- stale markers swept after teardown: `0`
- canonical Shared artifact present after teardown: `true`

## Samples

| cycle | MemAvailable before KiB | MemAvailable after KiB | delta KiB | bound KiB |
|---:|---:|---:|---:|---:|
| 1 | 8192 | 4096 | 4096 | 8192 |
| 2 | 8192 | 4096 | 4096 | 8192 |
| 3 | 8192 | 4096 | 4096 | 8192 |
| 4 | 8192 | 4096 | 4096 | 8192 |
| 5 | 8192 | 4096 | 4096 | 8192 |
| 6 | 8192 | 4096 | 4096 | 8192 |
| 7 | 8192 | 4096 | 4096 | 8192 |
| 8 | 8192 | 4096 | 4096 | 8192 |
| 9 | 8192 | 4096 | 4096 | 8192 |
| 10 | 8192 | 4096 | 4096 | 8192 |

## Payload erofs layout

The measured file is required to match the `.8.12` file-level DAX result: uncompressed, non-inlined erofs layout 0.

## Trust model

Shared pmem is admitted only with `TrustDomainAck` in the same trust domain.
The DAX cache-timing side channel is acknowledged by that trust model.
Shared pmem jail bindings are read-only; writable layers must use `PmemSharing::PerVm`.
"""
            .replace("{density_path}", density.as_posix())
        )
        density_smoke.write_text(
            """#!/usr/bin/env bash
set -euo pipefail

VM_COUNT="${M80_PMEM_SHARED_VM_COUNT:-4}"
CYCLES="${M80_PMEM_SHARED_CYCLES:-10}"
PAYLOAD_MIB="${M80_PMEM_SHARED_PAYLOAD_MIB:-128}"
PER_VM_OVERHEAD_KIB="${M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB:-131072}"
RUN_ROOT="${M80_RUN_ROOT:-/var/lib/m80-psd}"
ARTIFACT="${M80_PMEM_SHARED_DENSITY_ARTIFACT:-docs/perf/pmem-shared-density.md}"
ARTIFACT_PATH="$ARTIFACT"
ALLOW_OTHER_VMS="${M80_PMEM_SHARED_ALLOW_OTHER_VMS:-0}"
FIRECRACKER_BIN="${M80_FIRECRACKER_BIN:-/opt/firecracker/bin/firecracker}"
JAILER_BIN="${M80_JAILER_BIN:-/opt/firecracker/bin/jailer}"
KERNEL_KIND="${M80_KERNEL_KIND:-stripped}"
FIRECRACKER_SECCOMP_FILTER="${M80_FIRECRACKER_SECCOMP_FILTER:-/opt/firecracker/bin/firecracker-seccomp-filter.bin}"
JAILER_HARDEN_BIN="${M80_JAILER_HARDEN_BIN:-/opt/m80/bin/m80-jailer-harden}"
NET_HELPER_BIN="${M80_NET_HELPER_BIN:-/opt/m80/bin/m80-net-helper}"
KERNEL_IMAGE="${M80_KERNEL_IMAGE:-/var/lib/m80/kernels/vmlinux}"
ROOTFS_IMAGE="${M80_ROOTFS_IMAGE:-/var/lib/m80/rootfs.ext4}"
FIRECRACKER_VERSION="${M80_FIRECRACKER_VERSION:-v1.15.1}"
JAIL_UID="${M80_JAIL_UID:-1000}"
JAIL_GID="${M80_JAIL_GID:-1000}"
repro_command="M80_PMEM_SHARED_ALLOW_OTHER_VMS=$ALLOW_OTHER_VMS"
repro_command+=" M80_PMEM_SHARED_VM_COUNT=$VM_COUNT"
repro_command+=" M80_PMEM_SHARED_CYCLES=$CYCLES"
repro_command+=" M80_PMEM_SHARED_PAYLOAD_MIB=$PAYLOAD_MIB"
repro_command+=" M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=$PER_VM_OVERHEAD_KIB"
repro_command+=" M80_PMEM_SHARED_DENSITY_ARTIFACT=$ARTIFACT"
repro_command+=" M80_RUN_ROOT=$RUN_ROOT"
repro_command+=" M80_KERNEL_IMAGE=$KERNEL_IMAGE"
repro_command+=" M80_KERNEL_KIND=$KERNEL_KIND"
repro_command+=" M80_ROOTFS_IMAGE=$ROOTFS_IMAGE"
repro_command+=" M80_FIRECRACKER_BIN=$FIRECRACKER_BIN"
repro_command+=" M80_JAILER_BIN=$JAILER_BIN"
repro_command+=" M80_FIRECRACKER_SECCOMP_FILTER=$FIRECRACKER_SECCOMP_FILTER"
repro_command+=" M80_JAILER_HARDEN_BIN=$JAILER_HARDEN_BIN"
repro_command+=" M80_NET_HELPER_BIN=$NET_HELPER_BIN"
repro_command+=" M80_FIRECRACKER_VERSION=$FIRECRACKER_VERSION"
repro_command+=" M80_JAIL_UID=$JAIL_UID"
repro_command+=" M80_JAIL_GID=$JAIL_GID"
repro_command+=" $0"

if [[ "$ALLOW_OTHER_VMS" != "1" ]]; then
    existing_firecrackers="$(pgrep -af '(^|/)firecracker( |$)' || true)"
    if [[ -n "$existing_firecrackers" ]]; then
        echo "refusing density measurement while other Firecracker VMs are present" >&2
        exit 3
    fi
fi

sudo -n true
cargo test -p m80-firecracker --test pmem_shared_host_page_sharing_real_kvm --no-run
timeout 1800 sudo -n env \
    M80_RUN_PMEM_SHARED_DENSITY=1 \
    M80_PMEM_SHARED_VM_COUNT="$VM_COUNT" \
    M80_PMEM_SHARED_CYCLES="$CYCLES" \
    M80_PMEM_SHARED_PAYLOAD_MIB="$PAYLOAD_MIB" \
    M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB="$PER_VM_OVERHEAD_KIB" \
    M80_PMEM_SHARED_ALLOW_OTHER_VMS="$ALLOW_OTHER_VMS" \
    M80_PMEM_SHARED_DENSITY_ARTIFACT="$ARTIFACT_PATH" \
    M80_PMEM_SHARED_REPRO_COMMAND="$repro_command" \
    M80_FIRECRACKER_BIN="$FIRECRACKER_BIN" \
    M80_JAILER_BIN="$JAILER_BIN" \
    M80_KERNEL_KIND="$KERNEL_KIND" \
    M80_KERNEL_IMAGE="$KERNEL_IMAGE" \
    M80_ROOTFS_IMAGE="$ROOTFS_IMAGE" \
    M80_RUN_ROOT="$RUN_ROOT" \
    M80_FIRECRACKER_SECCOMP_FILTER="$FIRECRACKER_SECCOMP_FILTER" \
    M80_JAILER_HARDEN_BIN="$JAILER_HARDEN_BIN" \
    M80_NET_HELPER_BIN="$NET_HELPER_BIN" \
    M80_FIRECRACKER_VERSION="$FIRECRACKER_VERSION" \
    M80_JAIL_UID="$JAIL_UID" \
    M80_JAIL_GID="$JAIL_GID" \
    "$test_bin" shared_pmem_host_page_sharing_measurement_lives_in_density_gate \
    --ignored --exact --nocapture
"""
        )
        density_smoke.chmod(0o755)
        density_instruction = f"""# Q420K Close Gates

Prepared `vulcan` close inputs:

- stripped kernel:
  `{PREPARED_STRIPPED_KERNEL_IMAGE}`
- stripped kernel sha256:
  `{PREPARED_STRIPPED_KERNEL_SHA256}`
- rootfs:
  `{PREPARED_ROOTFS_IMAGE}`
- rootfs sha256:
  `{PREPARED_ROOTFS_SHA256}`
- Firecracker:
  `/opt/firecracker/bin/firecracker`
  `{PREPARED_FIRECRACKER_SHA256}`
  `Firecracker v1.15.1`
- jailer:
  `/opt/firecracker/bin/jailer`
  `{PREPARED_JAILER_SHA256}`
  `Jailer v1.15.1`
- seccomp filter:
  `/opt/firecracker/bin/firecracker-seccomp-filter.bin`
  `{PREPARED_SECCOMP_SHA256}`
- jailer harden helper:
  `/opt/m80/bin/m80-jailer-harden`
  `{PREPARED_JAILER_HARDEN_SHA256}`
- network helper:
  `/opt/m80/bin/m80-net-helper`
  `{PREPARED_NET_HELPER_SHA256}`

The rootfs manifest may still record the stock kernel used when that rootfs was
built. For q420k close-quality runs, the runtime kernel is the stripped kernel
above and the command must set `M80_KERNEL_KIND=stripped`. `m80-preflight`
applies that override to the emitted `preflight_artifacts`; verified-close
artifacts must report the stripped runtime kernel and the rootfs sha256 above.

## Q420K Shared Pmem Density

```sh
M80_PMEM_SHARED_ALLOW_OTHER_VMS=0 \
M80_PMEM_SHARED_VM_COUNT=4 \
M80_PMEM_SHARED_CYCLES=10 \
M80_PMEM_SHARED_PAYLOAD_MIB=128 \
M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=131072 \
M80_PMEM_SHARED_DENSITY_ARTIFACT=docs/perf/pmem-shared-density.md \
M80_RUN_ROOT=/var/lib/m80-psd \
M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
M80_JAILER_BIN=/opt/firecracker/bin/jailer \
M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin \
M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden \
M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper \
M80_KERNEL_IMAGE={PREPARED_STRIPPED_KERNEL_IMAGE} \
M80_KERNEL_KIND=stripped \
M80_ROOTFS_IMAGE={PREPARED_ROOTFS_IMAGE} \
M80_FIRECRACKER_VERSION=v1.15.1 \
M80_JAIL_UID=1000 \
M80_JAIL_GID=1000 \
./scripts/smoke-pmem-shared.sh
```

```sh
python3 scripts/verify-q420k-artifacts.py --only pmem-density --require-committed
```
"""
        composed_instruction = f"""
## E13. Composed E2E

```sh
sudo -n env \
  M80_COMPOSED_E2E_ALLOW_OTHER_VMS=0 \
  M80_COMPOSED_E2E_N=10 \
  M80_COMPOSED_E2E_SHARED_PAYLOAD_MIB=32 \
  M80_COMPOSED_E2E_OUT_DIR=/tank/projects/m80/crates/m80-firecracker/benches/snapshots \
  M80_RUN_ROOT=/var/lib/m80-composed-e2e \
  M80_JAIL_UID=1000 \
  M80_JAIL_GID=1000 \
  M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
  M80_JAILER_BIN=/opt/firecracker/bin/jailer \
  M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin \
  M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden \
  M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper \
  M80_ROOTFS_IMAGE={PREPARED_ROOTFS_IMAGE} \
  M80_KERNEL_IMAGE={PREPARED_STRIPPED_KERNEL_IMAGE} \
  M80_KERNEL_KIND=stripped \
  cargo test --release -p m80-firecracker --test e2e_composed_real_kvm -- \
    --ignored composed_e2e_layered_warm_pool --nocapture
```

- `crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json`
- `crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json`
- `crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json`
"""
        close_runbook.write_text(density_instruction)
        measurement_playbook.write_text(density_instruction + composed_instruction)
        quiet_host_inventory.write_text(
            """#!/usr/bin/env bash
set -euo pipefail

mapfile -t firecracker_pids < <(pgrep -x firecracker || true)
kubectl get pod -A -o json
systemctl --user status "tmux-spawn-example.scope" --no-pager --lines=0
echo "Refusing close-quality q420k measurement on this host until the listed processes are gone."
echo "This script is inventory only; it does not drain, signal, or kill anything."
exit 1
"""
        )
        quiet_host_inventory.chmod(0o755)
        restore.write_text(json.dumps({
            "schema_version": 1,
            "scenario": "composed_e2e_layered_warm_pool",
            "run_id": composed_run_id,
            "git_worktree_dirty_excluding_artifacts": False,
            "git_commit": git_commit,
            "page_cache_dropped_between_leases": False,
            "substrate": substrate,
            "data": {
                "restore_latency": {
                    "count": 10,
                    "target_ready": 10,
                    "samples_ms": [100.0 + index for index in range(10)],
                    "template_build_warmup_ms": [150.0],
                    "fail_count": 0,
                    "p50_ms": 104.0,
                    "p95_ms": 109.0,
                    "p99_ms": 109.0,
                },
                "warm_pool": {
                    "after_fill": {
                        "target_ready": 10,
                        "ready": 10,
                        "filling": 0,
                        "leased": 0,
                        "discarded": 0,
                        "consecutive_fill_errors": 0,
                        "fill_attempts_total": 10,
                        "fill_failures_total": 0,
                        "lease_acquired_total": 0,
                        "lease_returned_total": 0,
                    },
                    "during_leases": {
                        "target_ready": 10,
                        "ready": 0,
                        "filling": 0,
                        "leased": 10,
                        "discarded": 0,
                        "consecutive_fill_errors": 0,
                        "fill_attempts_total": 10,
                        "fill_failures_total": 0,
                        "lease_acquired_total": 10,
                        "lease_returned_total": 0,
                    },
                    "after_discard": {
                        "target_ready": 10,
                        "ready": 0,
                        "filling": 0,
                        "leased": 0,
                        "discarded": 10,
                        "consecutive_fill_errors": 0,
                        "fill_attempts_total": 10,
                        "fill_failures_total": 0,
                        "lease_acquired_total": 10,
                        "lease_returned_total": 0,
                    },
                },
                "observability": {
                    "diagnostics": [
                        {
                            "run_dir": f"{composed_run_root}/lease-{index}",
                            "vm_id": f"composed-{index}",
                            "phase_completed": [
                                "phase_3_storage_prep",
                                "phase_restore_load",
                                "phase_restore_probe_exec_channel",
                                "phase_restore_post_restore_hooks",
                            ],
                            "lifecycle_messages": [
                                "snapshot restored",
                                "restored guestd ready",
                            ],
                            "exec_completed_count": 5,
                        }
                        for index in range(10)
                    ],
                    "pmem_layers_by_sharing": {"Shared": 1, "PerVm": 1},
                },
            },
        }))
        memory.write_text(json.dumps({
            "schema_version": 1,
            "scenario": "composed_e2e_layered_warm_pool",
            "run_id": composed_run_id,
            "git_worktree_dirty_excluding_artifacts": False,
            "git_commit": git_commit,
            "page_cache_dropped_between_fill_and_attach": False,
            "substrate": substrate,
            "data": {"host_memory": {
                "n_attached": 10,
                "per_vm_overhead_source": "per_vm_baseline_same_run",
                "baseline_before_fill_bytes": 1000,
                "after_fill_bytes": 995,
                "after_n_attached_bytes": 990,
                "after_teardown_bytes": 1000,
                "after_n_attached_delta_bytes": 10,
                "shared_image_bytes": 10,
                "per_vm_overhead_bytes": 1,
                "bound_bytes": 20,
                "bound_satisfied": True,
                "shared_image_digest": shared_digest,
                "shared_image_path": f"/var/lib/m80-images/11/{shared_digest}/image.erofs",
                "shared_image_dev": 1,
                "shared_image_ino": 2,
                "shared_payload_layout": {
                    "erofs_layout": 0,
                    "size_bytes": 4096,
                    "on_disk_size_bytes": 4096,
                },
                "per_vm_baseline": {
                    "n_attached": 10,
                    "before_fill_bytes": 2000,
                    "after_fill_bytes": 1990,
                    "after_n_attached_bytes": 1890,
                    "after_teardown_bytes": 2000,
                    "after_n_attached_delta_bytes": 110,
                    "per_vm_payload_bytes": 100,
                    "per_vm_overhead_bytes": 1,
                    "image_digest": per_vm_digest,
                    "image_bytes": 10,
                    "shared_payload_digest": shared_digest,
                    "shared_payload_bytes": 10,
                    "template_fingerprint": "f" * 64,
                    "attached_snapshot": {
                        "target_ready": 10,
                        "ready": 0,
                        "filling": 0,
                        "leased": 10,
                        "discarded": 0,
                        "consecutive_fill_errors": 0,
                        "fill_attempts_total": 10,
                        "fill_failures_total": 0,
                        "lease_acquired_total": 10,
                        "lease_returned_total": 0,
                    },
                },
            }},
        }))
        residue.write_text(json.dumps({
            "schema_version": 1,
            "scenario": "composed_e2e_layered_warm_pool",
            "run_id": composed_run_id,
            "git_worktree_dirty_excluding_artifacts": False,
            "git_commit": git_commit,
            "substrate": substrate,
            "data": {"residue": {
                "n_leases": 10,
                "unexpected_paths": [],
                "scanned_roots": [
                    composed_run_root,
                    "/tmp/m80-*",
                    "/var/run/m80",
                    "/var/lib/m80-images",
                    composed_template_root,
                ],
                "leased_run_dirs": [f"{composed_run_root}/lease-{idx}" for idx in range(10)],
                "image_store": {
                    "expected": [shared_digest, per_vm_digest],
                    "preserved": [shared_digest, per_vm_digest],
                    "shared_digest": shared_digest,
                    "per_vm_digest": per_vm_digest,
                },
                "template_store": {"expected_fingerprint": "f" * 64, "preserved": ["f" * 64]},
            }},
        }))
        composed_doc.write_text(
            """# Composed E2E

## Method

Command:

```sh
sudo -n env \
  M80_COMPOSED_E2E_ALLOW_OTHER_VMS=0 \
  M80_COMPOSED_E2E_N=10 \
  M80_COMPOSED_E2E_SHARED_PAYLOAD_MIB=32 \
  M80_COMPOSED_E2E_OUT_DIR=/tank/projects/m80/crates/m80-firecracker/benches/snapshots \
  M80_RUN_ROOT=/var/lib/m80-composed-e2e \
  M80_JAIL_UID=1000 \
  M80_JAIL_GID=1000 \
  M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker \
  M80_JAILER_BIN=/opt/firecracker/bin/jailer \
  M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin \
  M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden \
  M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper \
  M80_ROOTFS_IMAGE=/var/lib/m80/rootfs.ext4 \
  M80_KERNEL_IMAGE=/var/lib/m80/kernels/vmlinux \
  M80_KERNEL_KIND=stripped \
  cargo test --release -p m80-firecracker --test e2e_composed_real_kvm -- \
    --ignored composed_e2e_layered_warm_pool --nocapture
```

Host: Linux 6.17.0 x86_64, Firecracker v1.15.1, real `/dev/kvm` rw,
real jailer, noninteractive sudo.

Substrate details:

- host kernel release: `6.17.0-23-generic`
- Firecracker version: `Firecracker v1.15.1`
- /dev/kvm stat: `crw-rw---- root:kvm /dev/kvm`
- sudo uid: `0`

Page cache was not dropped inside the run.

Measured git commit:
`cccccccccccccccccccccccccccccccccccccccc`

Run ID:
`composed-1a2b`

Preflight artifacts:

- firecracker_bin: `/opt/firecracker/bin/firecracker`
- firecracker_seccomp_filter: `/opt/firecracker/bin/firecracker-seccomp-filter.bin`
- jailer_bin: `/opt/firecracker/bin/jailer`
- jailer_harden_bin: `/opt/m80/bin/m80-jailer-harden`
- net_helper_bin: `/opt/m80/bin/m80-net-helper`
- kernel_image: `/var/lib/m80/kernels/vmlinux`
- rootfs_image: `/var/lib/m80/rootfs.ext4`
- expected_firecracker_version: `v1.15.1`
- kernel_image_sha256: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`
- rootfs_image_sha256: `bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb`

Shared image digest:
`1111111111111111111111111111111111111111111111111111111111111111`

Raw artifacts:

- `crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json`
- `crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json`
- `crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json`

## Restore latency

| N | fail count | P50 | P95 | P99 | bound |
|---:|---:|---:|---:|---:|---:|
| 10 | 0 | 104.000 ms | 109.000 ms | 109.000 ms | <= 200 ms |

## Host memory delta

| field | bytes |
|---|---:|
| composed `after_n_attached_delta_bytes` | 10 |
| Shared erofs image | 10 |
| derived `per_vm_overhead_bytes` | 1 |
| bound (`shared_image_bytes + per_vm_overhead_bytes * N`) | 20 |

Shared image digest: `1111111111111111111111111111111111111111111111111111111111111111`.

`after_n_attached_delta_bytes` stayed within bound.

## Residue

Shared image digest: `1111111111111111111111111111111111111111111111111111111111111111`.

Scanned roots:

- `/var/lib/m80-composed-e2e`
- `/tmp/m80-*`
- `/var/run/m80`
- `/var/lib/m80-images`
- `/var/lib/m80-composed-e2e/composed-e2e-templates-*/templates`

## Smoke evidence

```text
M80_COMPOSED_E2E_ARTIFACT /repo/crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json
M80_COMPOSED_E2E_ARTIFACT /repo/crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json
M80_COMPOSED_E2E_ARTIFACT /repo/crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json
test composed_e2e_layered_warm_pool ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 14 filtered out
```
"""
        )
        ext4_overlay.write_text(
            """# Ext4 Overlay-Template Clone Measurement

Bead: `m80-q420k.8.16`.

## Substrate

- host filesystem: `ext4` mounted at `/` from `/dev/nvme1n1p2`
- filesystem options: `rw,relatime`
- kernel: `6.17.0-23-generic`
- run root: `/var/tmp/m80-ext4-overlay-template-clone`
- artifact: `docs/perf/ext4-overlay-template-clone.md`
- commit: `cccccccccccccccccccccccccccccccccccccccc`
- git worktree dirty excluding this artifact: `false`
- substrate kind: `storage-only`
- command:

```sh
M80_RUN_EXT4_OVERLAY_TEMPLATE_CLONE=1 \
M80_EXT4_OVERLAY_TEMPLATE_SAMPLES=30 \
M80_EXT4_OVERLAY_TEMPLATE_RUN_ROOT=/var/tmp/m80-ext4-overlay-template-clone \
M80_EXT4_OVERLAY_TEMPLATE_ARTIFACT=docs/perf/ext4-overlay-template-clone.md \
cargo test -p m80-storage --test ext4_overlay_template_clone -- --ignored --nocapture
```

## Overlay Template

- overlay size bytes: `67108864`
- template logical size bytes: `67108864`
- template allocated bytes after hole digging: `4440064`
- clone mode: `byte-copy fallback` (`ext4` is classified non-reflink by the runtime gate)

## Observable

- samples: `30`
- phase_3b_rootfs_prepare p50_ms: `7.000`
- phase_3b_rootfs_prepare p95_ms: `8.000`
- phase_3b_rootfs_prepare p99_ms: `9.000`
- phase_3b_rootfs_prepare min_ms: `6.000`
- phase_3b_rootfs_prepare max_ms: `9.000`
- threshold result: `keep byte-copy fallback`

## Device-Mapper Comparison

dm-snapshot was not prototyped in this run.

- leaked dm devices: `0`

## Teardown Residue

- mount entries under run root before: `0`
- mount entries under run root after: `0`
- leaked mounts: `0`
- run root removed: `true`
"""
        )
        dax_pressure.write_text(
            f"""# Pmem DAX Memory Pressure

Bead: `m80-q420k.8.9`.

## Substrate

- substrate kind: `real-kvm`
- commit: `cccccccccccccccccccccccccccccccccccccccc`
- git worktree dirty excluding this artifact: `false`
- host kernel: `6.17.0-23-generic`
- host filesystem: `zfs`
- host memory: `128 GiB`
- pressure command: `stress-ng --vm 1 --vm-bytes 64G --timeout 30s`
- VM count: `2`
- samples per guest: `5`
- payload size: `32 MiB`
- image digest: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`
- image path: `/var/lib/m80-images/aa/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/image.erofs`
- payload erofs layout: `Layout: 0`, size `4096` bytes, on-disk size `4096` bytes
- Firecracker version: `v1.15.1`
- unrelated VMs running: `false`
- command:

```sh
M80_RUN_PMEM_DAX_MEMORY_PRESSURE=1 M80_PMEM_DAX_MEMORY_PRESSURE_COMMAND='stress-ng --vm 1 --vm-bytes 64G --timeout 30s' M80_PMEM_DAX_MEMORY_PRESSURE_VM_COUNT=2 M80_PMEM_DAX_MEMORY_PRESSURE_SAMPLES=5 M80_PMEM_DAX_MEMORY_PRESSURE_PAYLOAD_MIB=32 M80_PMEM_DAX_MEMORY_PRESSURE_ARTIFACT={dax_pressure.as_posix()} M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker M80_JAILER_BIN=/opt/firecracker/bin/jailer M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin M80_JAILER_HARDEN_BIN=/opt/m80/bin/m80-jailer-harden M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper M80_KERNEL_IMAGE=/var/lib/m80/kernels/vmlinux M80_KERNEL_KIND=stripped M80_ROOTFS_IMAGE=/var/lib/m80/rootfs.ext4 cargo test -p m80-firecracker --test pmem_dax_memory_pressure_real_kvm -- --ignored --nocapture
```

### Firecracker process substrate

```json
{json.dumps(substrate, indent=2)}
```

## Observable

- baseline Shared-payload read latency p50_ms: `1.000`
- baseline Shared-payload read latency p95_ms: `2.000`
- baseline Shared-payload read latency p99_ms: `3.000`
- post-pressure Shared-payload read latency p50_ms: `4.000`
- post-pressure Shared-payload read latency p95_ms: `5.000`
- post-pressure Shared-payload read latency p99_ms: `6.000`
- cross-guest signal delta p50_ms: `3.000`
- host memory delta during pressure bytes: `1000`
- host page-cache delta during pressure bytes: `2000`
- host memory delta after refault bytes: `3000`
- host page-cache delta after refault bytes: `4000`

## Teardown Residue

- leaked Shared markers: `0`
- leaked Firecracker/jailer processes: `0`
- leaked mounts: `0`

## Decision Output

The measured signal is acceptable under the same-trust-domain assumption.
"""
        )
        args = argparse.Namespace(
            snapshot_template=snapshot,
            snapshot_doc=snapshot_doc,
            pmem_density=density,
            pmem_density_smoke=density_smoke,
            close_runbook=close_runbook,
            measurement_playbook=measurement_playbook,
            quiet_host_inventory=quiet_host_inventory,
            composed_restore=restore,
            composed_memory=memory,
            composed_residue=residue,
            composed_doc=composed_doc,
            ext4_overlay=ext4_overlay,
            dax_memory_pressure=dax_pressure,
            only=None,
            require_committed=False,
        )
        ok_status = quiet_run_checks(args)
        args.only = ["composed-doc"]
        composed_doc_subset_status = quiet_run_checks(args)
        args.require_parent_phases_closed = True
        subset_parent_flag_status = quiet_run_checks(args)
        args.require_parent_phases_closed = False
        args.require_super_epic_closed = True
        subset_super_flag_status = quiet_run_checks(args)
        args.require_super_epic_closed = False
        args.only = None
        original_close_artifact_paths = REQUIRED_CLOSE_ARTIFACT_PATHS
        try:
            with tempfile.TemporaryDirectory(
                prefix=".q420k-close-path-self-test-",
                dir=ROOT,
            ) as repo_tmp_raw:
                repo_tmp = Path(repo_tmp_raw)
                (repo_tmp / ".gitignore").write_text("ignored.md\n")
                trackable_path = repo_tmp / "trackable.md"
                ignored_path = repo_tmp / "ignored.md"
                trackable_path.write_text("")
                ignored_path.write_text("")
                REQUIRED_CLOSE_ARTIFACT_PATHS = [trackable_path]
                close_artifact_paths_status = 0 if not verify_close_artifact_paths(ROOT) else 1
                REQUIRED_CLOSE_ARTIFACT_PATHS = [ignored_path]
                close_artifact_paths_ignored_status = 0 if not verify_close_artifact_paths(ROOT) else 1
        finally:
            REQUIRED_CLOSE_ARTIFACT_PATHS = original_close_artifact_paths
        args.only = ["ext4-overlay"]
        ext4_status = quiet_run_checks(args)
        ext4_bad = ext4_overlay.read_text().replace(
            "- phase_3b_rootfs_prepare p95_ms: `8.000`",
            "- phase_3b_rootfs_prepare p95_ms: `101.000`",
        )
        ext4_overlay.write_text(ext4_bad)
        ext4_bad_status = quiet_run_checks(args)
        ext4_overlay.write_text(ext4_bad.replace(
            "- phase_3b_rootfs_prepare p95_ms: `101.000`",
            "- phase_3b_rootfs_prepare p95_ms: `8.000`",
        ))
        ext4_bad_command = ext4_overlay.read_text().replace(
            "M80_EXT4_OVERLAY_TEMPLATE_RUN_ROOT=/var/tmp/m80-ext4-overlay-template-clone",
            "M80_EXT4_OVERLAY_TEMPLATE_RUN_ROOT=/var/tmp/m80-other-root",
        )
        ext4_overlay.write_text(ext4_bad_command)
        ext4_bad_command_status = quiet_run_checks(args)
        ext4_overlay.write_text(ext4_bad_command.replace(
            "M80_EXT4_OVERLAY_TEMPLATE_RUN_ROOT=/var/tmp/m80-other-root",
            "M80_EXT4_OVERLAY_TEMPLATE_RUN_ROOT=/var/tmp/m80-ext4-overlay-template-clone",
        ))
        args.only = ["dax-memory-pressure"]
        dax_status = quiet_run_checks(args)
        dax_bad = dax_pressure.read_text().replace(
            "- leaked mounts: `0`",
            "- leaked mounts: `1`",
        )
        dax_pressure.write_text(dax_bad)
        dax_bad_status = quiet_run_checks(args)
        dax_pressure.write_text(dax_bad.replace(
            "- leaked mounts: `1`",
            "- leaked mounts: `0`",
        ))
        dax_bad_repro = dax_pressure.read_text().replace(
            "M80_PMEM_DAX_MEMORY_PRESSURE_COMMAND='stress-ng --vm 1 --vm-bytes 64G --timeout 30s' ",
            "",
        )
        dax_pressure.write_text(dax_bad_repro)
        dax_bad_reproduction_command_status = quiet_run_checks(args)
        dax_pressure.write_text(
            dax_bad_repro.replace(
                "M80_RUN_PMEM_DAX_MEMORY_PRESSURE=1 ",
                "M80_RUN_PMEM_DAX_MEMORY_PRESSURE=1 "
                "M80_PMEM_DAX_MEMORY_PRESSURE_COMMAND='stress-ng --vm 1 --vm-bytes 64G --timeout 30s' ",
            )
        )
        dax_bad_digest = dax_pressure.read_text().replace(
            "- image digest: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`",
            "- image digest: `sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`",
        )
        dax_pressure.write_text(dax_bad_digest)
        dax_bad_digest_status = quiet_run_checks(args)
        dax_pressure.write_text(dax_bad_digest.replace(
            "- image digest: `sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`",
            "- image digest: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`",
        ))
        dax_bad_delta = dax_pressure.read_text().replace(
            "- cross-guest signal delta p50_ms: `3.000`",
            "- cross-guest signal delta p50_ms: `2.000`",
        )
        dax_pressure.write_text(dax_bad_delta)
        dax_bad_delta_status = quiet_run_checks(args)
        dax_pressure.write_text(dax_bad_delta.replace(
            "- cross-guest signal delta p50_ms: `2.000`",
            "- cross-guest signal delta p50_ms: `3.000`",
        ))
        dax_bad_substrate = dax_pressure.read_text().replace(
            '"allow_other_firecracker_vms": false',
            '"allow_other_firecracker_vms": true',
        )
        dax_pressure.write_text(dax_bad_substrate)
        dax_bad_substrate_status = quiet_run_checks(args)
        dax_pressure.write_text(dax_bad_substrate.replace(
            '"allow_other_firecracker_vms": true',
            '"allow_other_firecracker_vms": false',
        ))
        dax_bad_post_run = dax_pressure.read_text().replace(
            '"post_run_firecracker_processes": []',
            '"post_run_firecracker_processes": [{"pid": 1234, "argv": ["firecracker"]}]',
        )
        dax_pressure.write_text(dax_bad_post_run)
        dax_bad_post_run_status = quiet_run_checks(args)
        dax_pressure.write_text(dax_bad_post_run.replace(
            '"post_run_firecracker_processes": [{"pid": 1234, "argv": ["firecracker"]}]',
            '"post_run_firecracker_processes": []',
        ))
        dax_bad_preflight = dax_pressure.read_text().replace(
            '"preflight_artifacts": {',
            '"preflight_artifacts_missing": {',
        )
        dax_pressure.write_text(dax_bad_preflight)
        dax_bad_preflight_status = quiet_run_checks(args)
        dax_pressure.write_text(dax_bad_preflight.replace(
            '"preflight_artifacts_missing": {',
            '"preflight_artifacts": {',
        ))
        dax_bad_preflight_command = dax_pressure.read_text().replace(
            "M80_KERNEL_KIND=stripped",
            "M80_KERNEL_KIND=stock",
        )
        dax_pressure.write_text(dax_bad_preflight_command)
        dax_bad_preflight_command_status = quiet_run_checks(args)
        dax_pressure.write_text(dax_bad_preflight_command.replace(
            "M80_KERNEL_KIND=stock",
            "M80_KERNEL_KIND=stripped",
        ))
        args.only = ["snapshot-template"]
        snapshot_bad = json.loads(snapshot.read_text())
        snapshot_bad["substrate"]["substrate_kind"] = "mock"
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_substrate_status = quiet_run_checks(args)
        snapshot_bad["substrate"]["substrate_kind"] = "real-kvm"
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["substrate"]["post_run_firecracker_processes"] = [
            {"pid": 1234, "argv": ["firecracker"]},
        ]
        snapshot.write_text(json.dumps(snapshot_bad))
        leaked_firecracker_status = quiet_run_checks(args)
        snapshot_bad["substrate"]["post_run_firecracker_processes"] = []
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["substrate"]["host_kernel_release"] = "6.1.0"
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_host_kernel_status = quiet_run_checks(args)
        snapshot_bad["substrate"]["host_kernel_release"] = "6.17.0-23-generic"
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["substrate"]["dev_kvm_stat"] = "cr-------- root:kvm /dev/kvm"
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_kvm_status = quiet_run_checks(args)
        snapshot_bad["substrate"]["dev_kvm_stat"] = "crw-rw---- root:kvm /dev/kvm"
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["substrate"]["sudo_uid"] = "1000"
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_sudo_status = quiet_run_checks(args)
        snapshot_bad["substrate"]["sudo_uid"] = "0"
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["substrate"]["firecracker_version"] = "Firecracker v9.99.0"
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_firecracker_version_status = quiet_run_checks(args)
        snapshot_bad["substrate"]["firecracker_version"] = "Firecracker v1.15.1"
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["substrate"]["preflight_artifacts"]["kernel_image_sha256"] = "not-a-sha"
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_preflight_artifacts_status = quiet_run_checks(args)
        snapshot_bad["substrate"]["preflight_artifacts"]["kernel_image_sha256"] = "a" * 64
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["substrate"]["preflight_artifacts"]["kernel_kind"] = "stock"
        snapshot.write_text(json.dumps(snapshot_bad))
        stock_kernel_status = quiet_run_checks(args)
        snapshot_bad["substrate"]["preflight_artifacts"]["kernel_kind"] = "stripped"
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["git_commit"] = "short"
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_git_commit_status = quiet_run_checks(args)
        snapshot_bad["git_commit"] = git_commit
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["target_ready"] = 2
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_target_ready_status = quiet_run_checks(args)
        snapshot_bad["target_ready"] = 1
        snapshot.write_text(json.dumps(snapshot_bad))
        widened_n_per_run = SNAPSHOT_TEMPLATE_N_PER_RUN + 1
        widened_samples_total = widened_n_per_run * SNAPSHOT_TEMPLATE_RUNS
        widened_samples_us = [199_000 for _ in range(widened_samples_total)]
        snapshot_bad["n_per_run"] = widened_n_per_run
        snapshot_bad["samples_total"] = widened_samples_total
        snapshot_bad["data"]["warm"]["samples_us"] = widened_samples_us
        snapshot_bad["data"]["warm"]["sample_details"] = [
            {
                "run": index // widened_n_per_run,
                "cycle": index % widened_n_per_run,
                "fill_us": value - 1000,
                "handoff_us": 1000,
                "restore_to_handback_us": value,
            }
            for index, value in enumerate(widened_samples_us)
        ]
        snapshot_bad["runs_detail"] = [
            {
                "run": run,
                "initial_fill_samples_us": [50_000],
                "restore_to_handback_us": {"p99": 199_000},
                "samples_us": [199_000 for _ in range(widened_n_per_run)],
            }
            for run in range(SNAPSHOT_TEMPLATE_RUNS)
        ]
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_exact_shape_status = quiet_run_checks(args)
        snapshot_bad["n_per_run"] = SNAPSHOT_TEMPLATE_N_PER_RUN
        snapshot_bad["samples_total"] = SNAPSHOT_TEMPLATE_SAMPLES_TOTAL
        snapshot_bad["data"]["warm"]["samples_us"] = snapshot_samples_us
        snapshot_bad["data"]["warm"]["sample_details"] = [
            {
                "run": index // SNAPSHOT_TEMPLATE_N_PER_RUN,
                "cycle": index % SNAPSHOT_TEMPLATE_N_PER_RUN,
                "fill_us": value - 1000,
                "handoff_us": 1000,
                "restore_to_handback_us": value,
            }
            for index, value in enumerate(snapshot_samples_us)
        ]
        snapshot_bad["runs_detail"] = [
            {
                "run": run,
                "initial_fill_samples_us": [50_000],
                "restore_to_handback_us": {"p99": 199_000},
                "samples_us": [199_000 for _ in range(SNAPSHOT_TEMPLATE_N_PER_RUN)],
            }
            for run in range(SNAPSHOT_TEMPLATE_RUNS)
        ]
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["data"]["warm"]["samples_us"] = snapshot_bad["data"]["warm"]["samples_us"][:59]
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_sample_count_status = quiet_run_checks(args)
        snapshot_bad["data"]["warm"]["samples_us"] = snapshot_samples_us
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["data"]["warm"]["sample_details"][0]["restore_to_handback_us"] += 1
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_sample_detail_status = quiet_run_checks(args)
        snapshot_bad["data"]["warm"]["sample_details"][0]["restore_to_handback_us"] = snapshot_samples_us[0]
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["data"]["warm"]["sample_details"][1]["cycle"] = 0
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_run_cycle_status = quiet_run_checks(args)
        snapshot_bad["data"]["warm"]["sample_details"][1]["cycle"] = 1
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["runs_detail"][0]["samples_us"] = snapshot_bad["runs_detail"][0]["samples_us"][:19]
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_runs_detail_status = quiet_run_checks(args)
        snapshot_bad["runs_detail"][0]["samples_us"] = [
            199_000 for _ in range(SNAPSHOT_TEMPLATE_N_PER_RUN)
        ]
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["data"]["warm"]["restore_to_handback_ms"]["p99"] = 100.0
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_percentile_status = quiet_run_checks(args)
        snapshot_bad["data"]["warm"]["restore_to_handback_ms"]["p99"] = 199.0
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["reproduction_command"] = snapshot_bad["reproduction_command"].replace(
            "M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS=0 ",
            "",
        )
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_allow_other_reproduction_command_status = quiet_run_checks(args)
        snapshot_bad["reproduction_command"] = snapshot_bad["reproduction_command"].replace(
            "M80_SNAPSHOT_BENCH_LOAD=idle",
            "M80_SNAPSHOT_TEMPLATE_ALLOW_OTHER_VMS=0 M80_SNAPSHOT_BENCH_LOAD=idle",
        )
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["reproduction_command"] = snapshot_bad["reproduction_command"].replace(
            "N=20 ",
            "N=200 ",
        )
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_n_reproduction_command_status = quiet_run_checks(args)
        snapshot_bad["reproduction_command"] = snapshot_bad["reproduction_command"].replace(
            "N=200 ",
            "N=20 ",
        )
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["reproduction_command"] = snapshot_bad["reproduction_command"].replace(
            "M80_SNAPSHOT_TEMPLATE_RUNS=3 ",
            "M80_SNAPSHOT_TEMPLATE_RUNS=30 ",
        )
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_runs_reproduction_command_status = quiet_run_checks(args)
        snapshot_bad["reproduction_command"] = snapshot_bad["reproduction_command"].replace(
            "M80_SNAPSHOT_TEMPLATE_RUNS=30 ",
            "M80_SNAPSHOT_TEMPLATE_RUNS=3 ",
        )
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["reproduction_command"] = snapshot_bad["reproduction_command"].replace(
            "M80_KERNEL_KIND=stripped",
            "M80_KERNEL_KIND=stock",
        )
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_reproduction_command_status = quiet_run_checks(args)
        snapshot_bad["reproduction_command"] = snapshot_bad["reproduction_command"].replace(
            "M80_KERNEL_KIND=stock",
            "M80_KERNEL_KIND=stripped",
        )
        snapshot.write_text(json.dumps(snapshot_bad))
        snapshot_bad["reproduction_command"] = snapshot_bad["reproduction_command"].replace(
            "M80_JAIL_UID=1000 ",
            "",
        )
        snapshot.write_text(json.dumps(snapshot_bad))
        bad_snapshot_jail_uid_reproduction_command_status = quiet_run_checks(args)
        snapshot_bad["reproduction_command"] = snapshot_bad["reproduction_command"].replace(
            "M80_SNAPSHOT_TEMPLATE_BENCH_OUTPUT=",
            "M80_JAIL_UID=1000 M80_SNAPSHOT_TEMPLATE_BENCH_OUTPUT=",
        )
        snapshot.write_text(json.dumps(snapshot_bad))
        args.only = ["snapshot-doc"]
        snapshot_doc_bad = snapshot_doc.read_text().replace(
            "snapshot-template restore: load=idle runs=3 n=20 p99=199000us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
            "snapshot-template restore: load=idle runs=3 n=5 p99=199000us output=/tmp/diagnostic.json",
        )
        snapshot_doc.write_text(snapshot_doc_bad)
        snapshot_doc_bad_smoke_status = quiet_run_checks(args)
        snapshot_doc.write_text(snapshot_doc_bad.replace(
            "snapshot-template restore: load=idle runs=3 n=5 p99=199000us output=/tmp/diagnostic.json",
            "snapshot-template restore: load=idle runs=3 n=20 p99=199000us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
        ))
        snapshot_doc_duplicate_smoke = snapshot_doc.read_text().replace(
            "snapshot-template restore: load=idle runs=3 n=20 p99=199000us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
            "snapshot-template restore: load=idle runs=3 n=20 p99=199000us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json\n"
            "snapshot-template restore: load=idle runs=3 n=20 p99=198000us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
        )
        snapshot_doc.write_text(snapshot_doc_duplicate_smoke)
        snapshot_doc_duplicate_smoke_status = quiet_run_checks(args)
        snapshot_doc.write_text(snapshot_doc_duplicate_smoke.replace(
            "\nsnapshot-template restore: load=idle runs=3 n=20 p99=198000us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
            "",
        ))
        snapshot_doc_bad_kernel_kind = snapshot_doc.read_text().replace(
            "M80_KERNEL_KIND=stripped",
            "M80_KERNEL_KIND=stock",
        )
        snapshot_doc.write_text(snapshot_doc_bad_kernel_kind)
        snapshot_doc_bad_kernel_kind_status = quiet_run_checks(args)
        snapshot_doc.write_text(snapshot_doc_bad_kernel_kind.replace(
            "M80_KERNEL_KIND=stock",
            "M80_KERNEL_KIND=stripped",
        ))
        args.only = ["snapshot-template"]
        snapshot_doc_bad_command_identity = snapshot_doc.read_text().replace(
            "M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker",
            "M80_FIRECRACKER_BIN=/tmp/firecracker",
        )
        snapshot_doc.write_text(snapshot_doc_bad_command_identity)
        snapshot_doc_bad_command_identity_status = quiet_run_checks(args)
        snapshot_doc.write_text(snapshot_doc_bad_command_identity.replace(
            "M80_FIRECRACKER_BIN=/tmp/firecracker",
            "M80_FIRECRACKER_BIN=/opt/firecracker/bin/firecracker",
        ))
        args.only = ["snapshot-template"]
        snapshot_doc_bad_p99_scope = (
            snapshot_doc.read_text()
            .replace(
                "# Snapshot-Template Restore Latency\n",
                "# Snapshot-Template Restore Latency\n\nOutside smoke: p99=199000us.\n",
                1,
            )
            .replace(
                "snapshot-template restore: load=idle runs=3 n=20 p99=199000us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
                "snapshot-template restore: load=idle runs=3 n=20 p99=198000us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
            )
        )
        snapshot_doc.write_text(snapshot_doc_bad_p99_scope)
        snapshot_doc_bad_p99_scope_status = quiet_run_checks(args)
        snapshot_doc.write_text(
            snapshot_doc_bad_p99_scope
            .replace("\nOutside smoke: p99=199000us.\n", "", 1)
            .replace(
                "snapshot-template restore: load=idle runs=3 n=20 p99=198000us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
                "snapshot-template restore: load=idle runs=3 n=20 p99=199000us output=crates/m80-firecracker/benches/snapshot_template_restore_latency.json",
            )
        )
        snapshot_doc_bad_identity = snapshot_doc.read_text().replace(
            "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\n",
            "",
        )
        snapshot_doc.write_text(snapshot_doc_bad_identity)
        snapshot_doc_bad_identity_status = quiet_run_checks(args)
        snapshot_doc.write_text(
            snapshot_doc_bad_identity.replace(
                "Preflight artifacts:",
                "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\nPreflight artifacts:",
            )
        )
        snapshot_doc_identity_outside_section = (
            snapshot_doc.read_text()
            .replace(
                "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\n",
                "",
                1,
            )
            .replace(
                "# Snapshot-Template Restore Latency\n\n",
                "# Snapshot-Template Restore Latency\n\n"
                "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\n",
                1,
            )
        )
        snapshot_doc.write_text(snapshot_doc_identity_outside_section)
        snapshot_doc_identity_outside_section_status = quiet_run_checks(args)
        snapshot_doc.write_text(
            snapshot_doc_identity_outside_section
            .replace(
                "# Snapshot-Template Restore Latency\n\n"
                "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\n",
                "# Snapshot-Template Restore Latency\n\n",
                1,
            )
            .replace(
                "Preflight artifacts:",
                "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\n"
                "Preflight artifacts:",
                1,
            )
        )
        snapshot_doc_preflight_outside_section = (
            snapshot_doc.read_text()
            .replace(
                "- kernel_image_sha256: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`\n",
                "",
                1,
            )
            .replace(
                "# Snapshot-Template Restore Latency\n\n",
                "# Snapshot-Template Restore Latency\n\n"
                "- kernel_image_sha256: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`\n\n",
                1,
            )
        )
        snapshot_doc.write_text(snapshot_doc_preflight_outside_section)
        snapshot_doc_preflight_outside_section_status = quiet_run_checks(args)
        snapshot_doc.write_text(
            snapshot_doc_preflight_outside_section
            .replace(
                "# Snapshot-Template Restore Latency\n\n"
                "- kernel_image_sha256: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`\n\n",
                "# Snapshot-Template Restore Latency\n\n",
                1,
            )
            .replace(
                "Preflight artifacts:\n\n",
                "Preflight artifacts:\n\n"
                "- kernel_image_sha256: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`\n",
                1,
            )
        )
        snapshot_doc_runtime_outside_section = (
            snapshot_doc.read_text()
            .replace("- host_kernel_release: `6.17.0-23-generic`\n", "", 1)
            .replace(
                "# Snapshot-Template Restore Latency\n\n",
                "# Snapshot-Template Restore Latency\n\n"
                "- host_kernel_release: `6.17.0-23-generic`\n\n",
                1,
            )
        )
        snapshot_doc.write_text(snapshot_doc_runtime_outside_section)
        snapshot_doc_runtime_outside_section_status = quiet_run_checks(args)
        snapshot_doc.write_text(
            snapshot_doc_runtime_outside_section
            .replace(
                "# Snapshot-Template Restore Latency\n\n"
                "- host_kernel_release: `6.17.0-23-generic`\n\n",
                "# Snapshot-Template Restore Latency\n\n",
                1,
            )
            .replace(
                "Runtime substrate:\n\n",
                "Runtime substrate:\n\n- host_kernel_release: `6.17.0-23-generic`\n",
                1,
            )
        )
        args.only = ["pmem-density"]
        density_bad = density.read_text().replace(
            "- final active-use markers: `0`",
            "- final active-use markers: `1`",
        )
        density.write_text(density_bad)
        density_bad_teardown_status = quiet_run_checks(args)
        density.write_text(density_bad.replace(
            "- final active-use markers: `1`",
            "- final active-use markers: `0`",
        ))
        density_bad_host_kernel = density.read_text().replace(
            "- host kernel: `6.17.0-23-generic`",
            "- host kernel: `6.4.0`",
        )
        density.write_text(density_bad_host_kernel)
        density_bad_host_kernel_status = quiet_run_checks(args)
        density.write_text(density_bad_host_kernel.replace(
            "- host kernel: `6.4.0`",
            "- host kernel: `6.17.0-23-generic`",
        ))
        density_bad_kvm = density.read_text().replace(
            "- `/dev/kvm`: `crw-rw---- root:kvm /dev/kvm`",
            "- `/dev/kvm`: `cr-------- root:kvm /dev/kvm`",
        )
        density.write_text(density_bad_kvm)
        density_bad_kvm_status = quiet_run_checks(args)
        density.write_text(density_bad_kvm.replace(
            "- `/dev/kvm`: `cr-------- root:kvm /dev/kvm`",
            "- `/dev/kvm`: `crw-rw---- root:kvm /dev/kvm`",
        ))
        density_bad_sudo = density.read_text().replace(
            "- sudo: required; test ran as uid `0`",
            "- sudo: required; test ran as uid `1000`",
        )
        density.write_text(density_bad_sudo)
        density_bad_sudo_status = quiet_run_checks(args)
        density.write_text(density_bad_sudo.replace(
            "- sudo: required; test ran as uid `1000`",
            "- sudo: required; test ran as uid `0`",
        ))
        density_bad_substrate_runtime = density.read_text().replace(
            '"host_kernel_release": "6.17.0-23-generic"',
            '"host_kernel_release": "6.1.0"',
        )
        density.write_text(density_bad_substrate_runtime)
        density_bad_substrate_runtime_status = quiet_run_checks(args)
        density.write_text(density_bad_substrate_runtime.replace(
            '"host_kernel_release": "6.1.0"',
            '"host_kernel_release": "6.17.0-23-generic"',
        ))
        density_bad_bound = density.read_text().replace(
            "- bound: `8192 KiB`",
            "- bound: `8193 KiB`",
        )
        density.write_text(density_bad_bound)
        density_bad_bound_status = quiet_run_checks(args)
        density.write_text(density_bad_bound.replace(
            "- bound: `8193 KiB`",
            "- bound: `8192 KiB`",
        ))
        density_bad_digest = density.read_text().replace(
            "- image digest: `1111111111111111111111111111111111111111111111111111111111111111`",
            "- image digest: `sha256:1111111111111111111111111111111111111111111111111111111111111111`",
        )
        density.write_text(density_bad_digest)
        density_bad_digest_status = quiet_run_checks(args)
        density.write_text(density_bad_digest.replace(
            "- image digest: `sha256:1111111111111111111111111111111111111111111111111111111111111111`",
            "- image digest: `1111111111111111111111111111111111111111111111111111111111111111`",
        ))
        density_bad_image_path = density.read_text().replace(
            "- image path: `/var/lib/m80-images/11/1111111111111111111111111111111111111111111111111111111111111111/image.erofs`",
            "- image path: `/var/lib/m80-images/22/2222222222222222222222222222222222222222222222222222222222222222/image.erofs`",
        )
        density.write_text(density_bad_image_path)
        density_bad_image_path_status = quiet_run_checks(args)
        density.write_text(density_bad_image_path.replace(
            "- image path: `/var/lib/m80-images/22/2222222222222222222222222222222222222222222222222222222222222222/image.erofs`",
            "- image path: `/var/lib/m80-images/11/1111111111111111111111111111111111111111111111111111111111111111/image.erofs`",
        ))
        density_bad_preflight_command = density.read_text().replace(
            "M80_KERNEL_IMAGE=/var/lib/m80/kernels/vmlinux",
            "M80_KERNEL_IMAGE=/tmp/vmlinux",
        )
        density.write_text(density_bad_preflight_command)
        density_bad_preflight_command_status = quiet_run_checks(args)
        density.write_text(density_bad_preflight_command.replace(
            "M80_KERNEL_IMAGE=/tmp/vmlinux",
            "M80_KERNEL_IMAGE=/var/lib/m80/kernels/vmlinux",
        ))
        density_duplicate_substrate = density.read_text().replace(
            "### Firecracker process substrate",
            "### Firecracker process substrate\n\n```json\n"
            f"{json.dumps(substrate, indent=2)}\n"
            "```\n\n### Firecracker process substrate",
            1,
        )
        density.write_text(density_duplicate_substrate)
        density_duplicate_substrate_status = quiet_run_checks(args)
        density.write_text(density_duplicate_substrate.replace(
            "### Firecracker process substrate\n\n```json\n"
            f"{json.dumps(substrate, indent=2)}\n"
            "```\n\n### Firecracker process substrate",
            "### Firecracker process substrate",
            1,
        ))
        density_bad_sample = density.read_text().replace(
            "| 10 | 8192 | 4096 | 4096 | 8192 |",
            "| 10 | 8192 | 4096 | 4097 | 8192 |",
        )
        density.write_text(density_bad_sample)
        density_bad_sample_status = quiet_run_checks(args)
        density.write_text(density_bad_sample.replace(
            "| 10 | 8192 | 4096 | 4097 | 8192 |",
            "| 10 | 8192 | 4096 | 4096 | 8192 |",
        ))
        density_bad_exact_command = density.read_text().replace(
            "M80_PMEM_SHARED_ALLOW_OTHER_VMS=0 M80_PMEM_SHARED_VM_COUNT=4",
            "M80_PMEM_SHARED_ALLOW_OTHER_VMS=00 M80_PMEM_SHARED_VM_COUNT=40",
        )
        density.write_text(density_bad_exact_command)
        density_bad_exact_command_status = quiet_run_checks(args)
        density.write_text(density_bad_exact_command.replace(
            "M80_PMEM_SHARED_ALLOW_OTHER_VMS=00 M80_PMEM_SHARED_VM_COUNT=40",
            "M80_PMEM_SHARED_ALLOW_OTHER_VMS=0 M80_PMEM_SHARED_VM_COUNT=4",
        ))
        density_bad_repro = density.read_text().replace(
            "M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=1024 ",
            "",
        )
        density.write_text(density_bad_repro)
        density_bad_reproduction_command_status = quiet_run_checks(args)
        density.write_text(
            density_bad_repro.replace(
                "M80_PMEM_SHARED_DENSITY_ARTIFACT=",
                "M80_PMEM_SHARED_PER_VM_OVERHEAD_KIB=1024 M80_PMEM_SHARED_DENSITY_ARTIFACT=",
            )
        )
        density_command = markdown_text(density.read_text(), r"Command: `([^`]+)`")
        if density_command is None:
            density_command_outside_reproduction_status = 0
        else:
            density_command_outside_reproduction = (
                density.read_text()
                .replace(f"Command: `{density_command}`\n\n", "", 1)
                .replace(
                    "# Shared pmem density\n\n",
                    f"# Shared pmem density\n\nCommand: `{density_command}`\n\n",
                    1,
                )
            )
            density.write_text(density_command_outside_reproduction)
            density_command_outside_reproduction_status = quiet_run_checks(args)
            density.write_text(
                density_command_outside_reproduction
                .replace(f"Command: `{density_command}`\n\n", "", 1)
                .replace(
                    "## Reproduction\n\n",
                    f"## Reproduction\n\nCommand: `{density_command}`\n\n",
                    1,
                )
            )
        density_observable_outside_section = (
            density.read_text()
            .replace(
                "- image digest: `1111111111111111111111111111111111111111111111111111111111111111`\n",
                "",
                1,
            )
            .replace(
                "# Shared pmem density\n\n",
                "# Shared pmem density\n\n"
                "- image digest: `1111111111111111111111111111111111111111111111111111111111111111`\n\n",
                1,
            )
        )
        density.write_text(density_observable_outside_section)
        density_observable_outside_section_status = quiet_run_checks(args)
        density.write_text(
            density_observable_outside_section
            .replace(
                "# Shared pmem density\n\n"
                "- image digest: `1111111111111111111111111111111111111111111111111111111111111111`\n\n",
                "# Shared pmem density\n\n",
                1,
            )
            .replace(
                "- payload size: `128 MiB`\n",
                "- payload size: `128 MiB`\n"
                "- image digest: `1111111111111111111111111111111111111111111111111111111111111111`\n",
                1,
            )
        )
        density_bad_trust_section = (
            density.read_text()
            .replace(
                "## Trust model\n\nShared pmem is admitted only with `TrustDomainAck` in the same trust domain.\n",
                "Outside trust: Shared pmem is admitted only with `TrustDomainAck` in the same trust domain.\n\n## Trust model\n\n",
            )
        )
        density.write_text(density_bad_trust_section)
        density_bad_trust_section_status = quiet_run_checks(args)
        density.write_text(
            density_bad_trust_section.replace(
                "Outside trust: Shared pmem is admitted only with `TrustDomainAck` in the same trust domain.\n\n## Trust model\n\n",
                "## Trust model\n\nShared pmem is admitted only with `TrustDomainAck` in the same trust domain.\n",
            )
        )
        args.only = ["pmem-density-smoke"]
        density_smoke_status = quiet_run_checks(args)
        density_smoke_bad_repro_command = density_smoke.read_text().replace(
            'M80_PMEM_SHARED_REPRO_COMMAND="$repro_command"',
            'M80_PMEM_SHARED_REPRO_COMMAND_DISABLED="$repro_command"',
        )
        density_smoke.write_text(density_smoke_bad_repro_command)
        density_smoke_bad_repro_command_status = quiet_run_checks(args)
        density_smoke.write_text(density_smoke_bad_repro_command.replace(
            'M80_PMEM_SHARED_REPRO_COMMAND_DISABLED="$repro_command"',
            'M80_PMEM_SHARED_REPRO_COMMAND="$repro_command"',
        ))
        density_smoke_commented_guard = density_smoke.read_text().replace(
            'if [[ "$ALLOW_OTHER_VMS" != "1" ]]; then',
            '# if [[ "$ALLOW_OTHER_VMS" != "1" ]]; then',
        )
        density_smoke.write_text(density_smoke_commented_guard)
        density_smoke_commented_guard_status = quiet_run_checks(args)
        density_smoke.write_text(density_smoke_commented_guard.replace(
            '# if [[ "$ALLOW_OTHER_VMS" != "1" ]]; then',
            'if [[ "$ALLOW_OTHER_VMS" != "1" ]]; then',
        ))
        density_smoke.chmod(0o644)
        density_smoke_not_executable_status = quiet_run_checks(args)
        density_smoke.chmod(0o755)
        args.only = ["pmem-density-runbook"]
        density_runbook_status = quiet_run_checks(args)
        density_runbook_bad_allow_other = close_runbook.read_text().replace(
            "M80_PMEM_SHARED_ALLOW_OTHER_VMS=0 ",
            "",
        )
        close_runbook.write_text(density_runbook_bad_allow_other)
        density_runbook_bad_allow_other_status = quiet_run_checks(args)
        close_runbook.write_text(density_runbook_bad_allow_other.replace(
            "M80_PMEM_SHARED_VM_COUNT=4",
            "M80_PMEM_SHARED_ALLOW_OTHER_VMS=0 M80_PMEM_SHARED_VM_COUNT=4",
        ))
        density_runbook_bad = close_runbook.read_text().replace(
            "M80_KERNEL_KIND=stripped",
            "M80_KERNEL_KIND=stock",
        )
        close_runbook.write_text(density_runbook_bad)
        density_runbook_bad_kernel_kind_status = quiet_run_checks(args)
        close_runbook.write_text(density_runbook_bad.replace(
            "M80_KERNEL_KIND=stock",
            "M80_KERNEL_KIND=stripped",
        ))
        density_runbook_bad_prepared_sha = close_runbook.read_text().replace(
            PREPARED_ROOTFS_SHA256,
            "0" * 64,
            1,
        )
        close_runbook.write_text(density_runbook_bad_prepared_sha)
        density_runbook_bad_prepared_sha_status = quiet_run_checks(args)
        close_runbook.write_text(density_runbook_bad_prepared_sha.replace(
            "0" * 64,
            PREPARED_ROOTFS_SHA256,
            1,
        ))
        density_runbook_bad_firecracker_sha = close_runbook.read_text().replace(
            PREPARED_FIRECRACKER_SHA256,
            "1" * 64,
            1,
        )
        close_runbook.write_text(density_runbook_bad_firecracker_sha)
        density_runbook_bad_firecracker_sha_status = quiet_run_checks(args)
        close_runbook.write_text(density_runbook_bad_firecracker_sha.replace(
            "1" * 64,
            PREPARED_FIRECRACKER_SHA256,
            1,
        ))
        density_runbook_outside_command = (
            close_runbook.read_text()
            .replace("M80_RUN_ROOT=/var/lib/m80-psd ", "", 1)
            .replace(
                "## Q420K Shared Pmem Density\n\n",
                "M80_RUN_ROOT=/var/lib/m80-psd\n\n## Q420K Shared Pmem Density\n\n",
                1,
            )
        )
        close_runbook.write_text(density_runbook_outside_command)
        density_runbook_outside_command_status = quiet_run_checks(args)
        close_runbook.write_text(
            density_runbook_outside_command
            .replace("M80_RUN_ROOT=/var/lib/m80-psd\n\n", "", 1)
            .replace(
                "M80_PMEM_SHARED_DENSITY_ARTIFACT=docs/perf/pmem-shared-density.md ",
                "M80_PMEM_SHARED_DENSITY_ARTIFACT=docs/perf/pmem-shared-density.md "
                "M80_RUN_ROOT=/var/lib/m80-psd ",
                1,
            )
        )
        args.only = ["quiet-host-inventory"]
        quiet_host_inventory_status = quiet_run_checks(args)
        quiet_host_inventory.chmod(0o644)
        quiet_host_inventory_not_executable_status = quiet_run_checks(args)
        quiet_host_inventory.chmod(0o755)
        quiet_host_inventory_bad = quiet_host_inventory.read_text().replace(
            "exit 1",
            'kill -9 "$pid"\nexit 1',
        )
        quiet_host_inventory.write_text(quiet_host_inventory_bad)
        quiet_host_inventory_mutating_status = quiet_run_checks(args)
        quiet_host_inventory.write_text(quiet_host_inventory_bad.replace(
            'kill -9 "$pid"\n',
            "",
        ))
        args.only = ["composed-restore"]
        restore_bad = json.loads(restore.read_text())
        restore_bad["page_cache_dropped_between_leases"] = True
        restore.write_text(json.dumps(restore_bad))
        restore_bad_page_cache_status = quiet_run_checks(args)
        restore_bad["page_cache_dropped_between_leases"] = False
        restore.write_text(json.dumps(restore_bad))
        restore_bad.pop("run_id")
        restore.write_text(json.dumps(restore_bad))
        restore_bad_run_id_status = quiet_run_checks(args)
        restore_bad["run_id"] = composed_run_id
        restore.write_text(json.dumps(restore_bad))
        restore_bad["substrate"]["host_kernel_release"] = "6.1.0"
        restore.write_text(json.dumps(restore_bad))
        restore_bad_host_kernel_status = quiet_run_checks(args)
        restore_bad["substrate"]["host_kernel_release"] = "6.17.0-23-generic"
        restore.write_text(json.dumps(restore_bad))
        restore_bad["substrate"]["dev_kvm_stat"] = "cr-------- root:kvm /dev/kvm"
        restore.write_text(json.dumps(restore_bad))
        restore_bad_kvm_status = quiet_run_checks(args)
        restore_bad["substrate"]["dev_kvm_stat"] = "crw-rw---- root:kvm /dev/kvm"
        restore.write_text(json.dumps(restore_bad))
        restore_bad["substrate"]["sudo_uid"] = "1000"
        restore.write_text(json.dumps(restore_bad))
        restore_bad_sudo_status = quiet_run_checks(args)
        restore_bad["substrate"]["sudo_uid"] = "0"
        restore.write_text(json.dumps(restore_bad))
        restore_bad["substrate"]["firecracker_version"] = "Firecracker v9.99.0"
        restore.write_text(json.dumps(restore_bad))
        restore_bad_firecracker_version_status = quiet_run_checks(args)
        restore_bad["substrate"]["firecracker_version"] = "Firecracker v1.15.1"
        restore.write_text(json.dumps(restore_bad))
        restore_bad["data"]["restore_latency"]["target_ready"] = 9
        restore.write_text(json.dumps(restore_bad))
        restore_bad_target_ready_status = quiet_run_checks(args)
        restore_bad["data"]["restore_latency"]["target_ready"] = 10
        restore.write_text(json.dumps(restore_bad))
        restore_bad["data"]["restore_latency"]["count"] = 11
        restore_bad["data"]["restore_latency"]["target_ready"] = 11
        restore_bad["data"]["restore_latency"]["samples_ms"] = [100.0 + index for index in range(11)]
        restore_bad["data"]["restore_latency"]["p50_ms"] = 105.0
        restore_bad["data"]["restore_latency"]["p95_ms"] = 110.0
        restore_bad["data"]["restore_latency"]["p99_ms"] = 110.0
        restore.write_text(json.dumps(restore_bad))
        restore_bad_exact_n_status = quiet_run_checks(args)
        restore_bad["data"]["restore_latency"]["count"] = 10
        restore_bad["data"]["restore_latency"]["target_ready"] = 10
        restore_bad["data"]["restore_latency"]["samples_ms"] = [100.0 + index for index in range(10)]
        restore_bad["data"]["restore_latency"]["p50_ms"] = 104.0
        restore_bad["data"]["restore_latency"]["p95_ms"] = 109.0
        restore_bad["data"]["restore_latency"]["p99_ms"] = 109.0
        restore.write_text(json.dumps(restore_bad))
        restore_bad["data"]["restore_latency"]["samples_ms"] = restore_bad["data"]["restore_latency"]["samples_ms"][:9]
        restore.write_text(json.dumps(restore_bad))
        restore_bad_sample_count_status = quiet_run_checks(args)
        restore_bad["data"]["restore_latency"]["samples_ms"] = [100.0 + index for index in range(10)]
        restore.write_text(json.dumps(restore_bad))
        restore_bad["data"]["restore_latency"]["p99_ms"] = 100.0
        restore.write_text(json.dumps(restore_bad))
        restore_bad_percentile_status = quiet_run_checks(args)
        restore_bad["data"]["restore_latency"]["p99_ms"] = 109.0
        restore.write_text(json.dumps(restore_bad))
        restore_bad["data"]["warm_pool"]["during_leases"]["leased"] = 9
        restore.write_text(json.dumps(restore_bad))
        restore_bad_warm_pool_status = quiet_run_checks(args)
        restore_bad["data"]["warm_pool"]["during_leases"]["leased"] = 10
        restore.write_text(json.dumps(restore_bad))
        restore_bad["data"]["observability"]["diagnostics"][0]["phase_completed"].remove(
            "phase_restore_post_restore_hooks"
        )
        restore.write_text(json.dumps(restore_bad))
        restore_bad_diagnostics_phase_status = quiet_run_checks(args)
        restore_bad["data"]["observability"]["diagnostics"][0]["phase_completed"].append(
            "phase_restore_post_restore_hooks"
        )
        restore.write_text(json.dumps(restore_bad))
        restore_bad["data"]["observability"]["diagnostics"][0]["exec_completed_count"] = 4
        restore.write_text(json.dumps(restore_bad))
        restore_bad_exec_count_status = quiet_run_checks(args)
        restore_bad["data"]["observability"]["diagnostics"][0]["exec_completed_count"] = 5
        restore.write_text(json.dumps(restore_bad))
        restore_bad["data"]["observability"]["pmem_layers_by_sharing"]["Shared"] = 0
        restore.write_text(json.dumps(restore_bad))
        restore_bad_pmem_layers_status = quiet_run_checks(args)
        restore_bad["data"]["observability"]["pmem_layers_by_sharing"]["Shared"] = 1
        restore.write_text(json.dumps(restore_bad))
        args.only = ["composed-memory"]
        memory_bad_bound = json.loads(memory.read_text())
        memory_bad_bound["data"]["host_memory"]["bound_bytes"] = 21
        memory.write_text(json.dumps(memory_bad_bound))
        memory_bad_bound_status = quiet_run_checks(args)
        memory_bad_bound["data"]["host_memory"]["bound_bytes"] = 20
        memory.write_text(json.dumps(memory_bad_bound))
        memory_bad_n = json.loads(memory.read_text())
        memory_bad_n["data"]["host_memory"]["n_attached"] = 11
        memory_bad_n["data"]["host_memory"]["per_vm_baseline"]["n_attached"] = 11
        memory_bad_n["data"]["host_memory"]["per_vm_baseline"]["attached_snapshot"]["target_ready"] = 11
        memory_bad_n["data"]["host_memory"]["per_vm_baseline"]["attached_snapshot"]["leased"] = 11
        memory.write_text(json.dumps(memory_bad_n))
        memory_bad_n_status = quiet_run_checks(args)
        memory_bad_n["data"]["host_memory"]["n_attached"] = 10
        memory_bad_n["data"]["host_memory"]["per_vm_baseline"]["n_attached"] = 10
        memory_bad_n["data"]["host_memory"]["per_vm_baseline"]["attached_snapshot"]["target_ready"] = 10
        memory_bad_n["data"]["host_memory"]["per_vm_baseline"]["attached_snapshot"]["leased"] = 10
        memory.write_text(json.dumps(memory_bad_n))
        memory_bad_digest = json.loads(memory.read_text())
        memory_bad_digest["data"]["host_memory"]["shared_image_digest"] = f"sha256:{shared_digest}"
        memory.write_text(json.dumps(memory_bad_digest))
        memory_bad_digest_status = quiet_run_checks(args)
        memory_bad_digest["data"]["host_memory"]["shared_image_digest"] = shared_digest
        memory.write_text(json.dumps(memory_bad_digest))
        memory_bad_delta = json.loads(memory.read_text())
        memory_bad_delta["data"]["host_memory"]["after_n_attached_delta_bytes"] = 9
        memory.write_text(json.dumps(memory_bad_delta))
        memory_bad_delta_status = quiet_run_checks(args)
        memory_bad_delta["data"]["host_memory"]["after_n_attached_delta_bytes"] = 10
        memory.write_text(json.dumps(memory_bad_delta))
        memory_bad_path = json.loads(memory.read_text())
        memory_bad_path["data"]["host_memory"]["shared_image_path"] = (
            "/var/lib/m80-images/22/"
            "2222222222222222222222222222222222222222222222222222222222222222/image.erofs"
        )
        memory.write_text(json.dumps(memory_bad_path))
        memory_bad_path_status = quiet_run_checks(args)
        memory_bad_path["data"]["host_memory"]["shared_image_path"] = (
            f"/var/lib/m80-images/11/{shared_digest}/image.erofs"
        )
        memory.write_text(json.dumps(memory_bad_path))
        memory_bad_per_vm_baseline = json.loads(memory.read_text())
        memory_bad_per_vm_baseline["data"]["host_memory"]["per_vm_baseline"]["shared_payload_digest"] = (
            per_vm_digest
        )
        memory.write_text(json.dumps(memory_bad_per_vm_baseline))
        memory_bad_per_vm_baseline_status = quiet_run_checks(args)
        memory_bad_per_vm_baseline["data"]["host_memory"]["per_vm_baseline"]["shared_payload_digest"] = (
            shared_digest
        )
        memory.write_text(json.dumps(memory_bad_per_vm_baseline))
        memory_bad_per_vm_baseline_overhead = json.loads(memory.read_text())
        memory_bad_per_vm_baseline_overhead["data"]["host_memory"]["per_vm_baseline"][
            "after_n_attached_bytes"
        ] = 1880
        memory_bad_per_vm_baseline_overhead["data"]["host_memory"]["per_vm_baseline"][
            "after_n_attached_delta_bytes"
        ] = 120
        memory.write_text(json.dumps(memory_bad_per_vm_baseline_overhead))
        memory_bad_per_vm_baseline_overhead_status = quiet_run_checks(args)
        memory_bad_per_vm_baseline_overhead["data"]["host_memory"]["per_vm_baseline"][
            "after_n_attached_bytes"
        ] = 1890
        memory_bad_per_vm_baseline_overhead["data"]["host_memory"]["per_vm_baseline"][
            "after_n_attached_delta_bytes"
        ] = 110
        memory.write_text(json.dumps(memory_bad_per_vm_baseline_overhead))
        args.only = ["composed-residue"]
        residue_bad = json.loads(residue.read_text())
        residue_bad["data"]["residue"]["image_store"]["shared_digest"] = f"sha256:{shared_digest}"
        residue.write_text(json.dumps(residue_bad))
        residue_bad_digest_status = quiet_run_checks(args)
        residue_bad["data"]["residue"]["image_store"]["shared_digest"] = shared_digest
        residue.write_text(json.dumps(residue_bad))
        residue_bad["data"]["residue"]["image_store"]["expected"] = [shared_digest]
        residue_bad["data"]["residue"]["image_store"]["preserved"] = [shared_digest]
        residue.write_text(json.dumps(residue_bad))
        residue_bad_image_expected_status = quiet_run_checks(args)
        residue_bad["data"]["residue"]["image_store"]["expected"] = [shared_digest, per_vm_digest]
        residue_bad["data"]["residue"]["image_store"]["preserved"] = [shared_digest, per_vm_digest]
        residue.write_text(json.dumps(residue_bad))
        residue_bad["data"]["residue"]["template_store"]["expected_fingerprint"] = "not-a-fingerprint"
        residue_bad["data"]["residue"]["template_store"]["preserved"] = ["not-a-fingerprint"]
        residue.write_text(json.dumps(residue_bad))
        residue_bad_template_fingerprint_status = quiet_run_checks(args)
        residue_bad["data"]["residue"]["template_store"]["expected_fingerprint"] = "f" * 64
        residue_bad["data"]["residue"]["template_store"]["preserved"] = ["f" * 64]
        residue.write_text(json.dumps(residue_bad))
        residue_bad["data"]["residue"]["n_leases"] = 11
        residue_bad["data"]["residue"]["leased_run_dirs"] = [
            f"{composed_run_root}/lease-{idx}" for idx in range(11)
        ]
        residue.write_text(json.dumps(residue_bad))
        residue_bad_exact_n_status = quiet_run_checks(args)
        residue_bad["data"]["residue"]["n_leases"] = 10
        residue_bad["data"]["residue"]["leased_run_dirs"] = [
            f"{composed_run_root}/lease-{idx}" for idx in range(10)
        ]
        residue.write_text(json.dumps(residue_bad))
        residue_bad["data"]["residue"]["scanned_roots"] = ["/tmp/m80-*"]
        residue.write_text(json.dumps(residue_bad))
        residue_bad_roots_status = quiet_run_checks(args)
        residue_bad["data"]["residue"]["scanned_roots"] = [
            composed_run_root,
            "/tmp/m80-*",
            "/var/run/m80",
            "/var/lib/m80-images",
            composed_template_root,
        ]
        residue.write_text(json.dumps(residue_bad))
        residue_bad["data"]["residue"]["scanned_roots"] = [
            "/tmp/m80-*",
            "/var/run/m80",
            "/var/lib/m80-images",
            composed_template_root,
            "/var/lib/m80-other/templates",
        ]
        residue.write_text(json.dumps(residue_bad))
        residue_bad_run_root_status = quiet_run_checks(args)
        residue_bad["data"]["residue"]["scanned_roots"] = [
            composed_run_root,
            "/tmp/m80-*",
            "/var/run/m80",
            "/var/lib/m80-images",
            composed_template_root,
        ]
        residue.write_text(json.dumps(residue_bad))
        residue_bad["data"]["residue"]["leased_run_dirs"][0] = "relative-lease-dir"
        residue.write_text(json.dumps(residue_bad))
        residue_bad_leased_run_dir_status = quiet_run_checks(args)
        residue_bad["data"]["residue"]["leased_run_dirs"][0] = f"{composed_run_root}/lease-0"
        residue.write_text(json.dumps(residue_bad))
        residue_bad["data"]["residue"]["leased_run_dirs"][0] = "/var/lib/m80-other/lease-0"
        residue.write_text(json.dumps(residue_bad))
        residue_bad_leased_run_dir_root_status = quiet_run_checks(args)
        residue_bad["data"]["residue"]["leased_run_dirs"][0] = f"{composed_run_root}/lease-0"
        residue.write_text(json.dumps(residue_bad))
        args.only = ["composed-restore", "composed-memory", "composed-residue"]
        memory_bad = json.loads(memory.read_text())
        memory_bad["git_commit"] = "d" * 40
        memory.write_text(json.dumps(memory_bad))
        composed_bad_consistency_status = quiet_run_checks(args)
        memory_bad["git_commit"] = git_commit
        memory.write_text(json.dumps(memory_bad))
        memory_bad["run_id"] = "composed-ffff"
        memory.write_text(json.dumps(memory_bad))
        composed_bad_run_id_consistency_status = quiet_run_checks(args)
        memory_bad["run_id"] = composed_run_id
        memory.write_text(json.dumps(memory_bad))
        args.only = None
        args.require_committed = True
        uncommitted_status = quiet_run_checks(args)
        args.require_committed = False
        composed_doc.write_text(
            composed_doc.read_text().replace(
                "# Composed E2E\n",
                "# Composed E2E\n\n> Status: scaffold and noisy-host diagnostic only.\n",
            )
        )
        diagnostic_doc_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc.read_text().replace(
                "\n> Status: scaffold and noisy-host diagnostic only.\n",
                "",
            )
        )
        composed_doc_bad_artifact_marker = composed_doc.read_text().replace(
            "M80_COMPOSED_E2E_ARTIFACT /repo/crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json",
            "M80_COMPOSED_E2E_ARTIFACT /tmp/composed-e2e-residue.json",
        )
        composed_doc.write_text(composed_doc_bad_artifact_marker)
        composed_doc_bad_artifact_marker_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc_bad_artifact_marker.replace(
                "M80_COMPOSED_E2E_ARTIFACT /tmp/composed-e2e-residue.json",
                "M80_COMPOSED_E2E_ARTIFACT /repo/crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json",
            )
        )
        composed_doc_smoke_outside_section = (
            "M80_COMPOSED_E2E_ARTIFACT /repo/crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json\n\n"
            + composed_doc.read_text().replace(
                "```text\nM80_COMPOSED_E2E_ARTIFACT /repo/crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json\n",
                "```text\n",
                1,
            )
        )
        composed_doc.write_text(composed_doc_smoke_outside_section)
        composed_doc_smoke_outside_section_status = quiet_run_checks(args)
        composed_doc.write_text(composed_doc_smoke_outside_section.split("\n\n", maxsplit=1)[1].replace(
            "```text\n",
            "```text\nM80_COMPOSED_E2E_ARTIFACT /repo/crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json\n",
            1,
        ))
        composed_doc_bad = composed_doc.read_text().replace(
            "cargo test --release -p m80-firecracker --test e2e_composed_real_kvm",
            "cargo test --release -p m80-firecracker --test wrong_test",
        )
        composed_doc.write_text(composed_doc_bad)
        missing_doc_command_status = quiet_run_checks(args)
        composed_doc.write_text(composed_doc_bad.replace(
            "cargo test --release -p m80-firecracker --test wrong_test",
            "cargo test --release -p m80-firecracker --test e2e_composed_real_kvm",
        ))
        composed_doc_bad_helper = composed_doc.read_text().replace(
            "M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper",
            "M80_NET_HELPER_BIN=/tmp/m80-net-helper",
        )
        composed_doc.write_text(composed_doc_bad_helper)
        composed_doc_bad_helper_status = quiet_run_checks(args)
        composed_doc.write_text(composed_doc_bad_helper.replace(
            "M80_NET_HELPER_BIN=/tmp/m80-net-helper",
            "M80_NET_HELPER_BIN=/opt/m80/bin/m80-net-helper",
        ))
        composed_doc_bad_run_root = composed_doc.read_text().replace(
            "M80_RUN_ROOT=/var/lib/m80-composed-e2e",
            "M80_RUN_ROOT=/tmp/m80-composed-e2e",
        )
        composed_doc.write_text(composed_doc_bad_run_root)
        composed_doc_bad_run_root_status = quiet_run_checks(args)
        composed_doc.write_text(composed_doc_bad_run_root.replace(
            "M80_RUN_ROOT=/tmp/m80-composed-e2e",
            "M80_RUN_ROOT=/var/lib/m80-composed-e2e",
        ))
        composed_doc_bad_kernel_kind = composed_doc.read_text().replace(
            "M80_KERNEL_KIND=stripped",
            "M80_KERNEL_KIND=stock",
        )
        composed_doc.write_text(composed_doc_bad_kernel_kind)
        composed_doc_bad_kernel_kind_status = quiet_run_checks(args)
        composed_doc.write_text(composed_doc_bad_kernel_kind.replace(
            "M80_KERNEL_KIND=stock",
            "M80_KERNEL_KIND=stripped",
        ))
        args.only = ["composed-playbook"]
        composed_playbook_status = quiet_run_checks(args)
        composed_playbook_bad = measurement_playbook.read_text().replace(
            "M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin",
            "M80_FIRECRACKER_SECCOMP_FILTER=/tmp/filter.bin",
        )
        measurement_playbook.write_text(composed_playbook_bad)
        composed_playbook_bad_helper_status = quiet_run_checks(args)
        measurement_playbook.write_text(composed_playbook_bad.replace(
            "M80_FIRECRACKER_SECCOMP_FILTER=/tmp/filter.bin",
            "M80_FIRECRACKER_SECCOMP_FILTER=/opt/firecracker/bin/firecracker-seccomp-filter.bin",
        ))
        composed_playbook_bad_run_root = measurement_playbook.read_text().replace(
            "M80_RUN_ROOT=/var/lib/m80-composed-e2e",
            "M80_RUN_ROOT=/tmp/m80-composed-e2e",
        )
        measurement_playbook.write_text(composed_playbook_bad_run_root)
        composed_playbook_bad_run_root_status = quiet_run_checks(args)
        measurement_playbook.write_text(composed_playbook_bad_run_root.replace(
            "M80_RUN_ROOT=/tmp/m80-composed-e2e",
            "M80_RUN_ROOT=/var/lib/m80-composed-e2e",
        ))
        composed_playbook_artifact_outside_section = (
            measurement_playbook.read_text()
            .replace(
                "- `crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json`\n",
                "",
                1,
            )
            .replace(
                "# Perf measurement playbook\n\n",
                "# Perf measurement playbook\n\n"
                "- `crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json`\n\n",
                1,
            )
        )
        measurement_playbook.write_text(composed_playbook_artifact_outside_section)
        composed_playbook_artifact_outside_section_status = quiet_run_checks(args)
        measurement_playbook.write_text(
            composed_playbook_artifact_outside_section
            .replace(
                "# Perf measurement playbook\n\n"
                "- `crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json`\n\n",
                "# Perf measurement playbook\n\n",
                1,
            )
            .replace(
                "- `crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json`\n",
                "- `crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json`\n"
                "- `crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json`\n",
                1,
            )
        )
        args.only = ["composed-doc"]
        composed_doc_bad = composed_doc.read_text().replace(
            "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\n",
            "",
        )
        composed_doc.write_text(composed_doc_bad)
        missing_doc_json_identity_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc_bad.replace(
                "Preflight artifacts:",
                "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\nPreflight artifacts:",
            )
        )
        composed_doc_missing_run_id = composed_doc.read_text().replace(
            "Run ID:\n`composed-1a2b`\n\n",
            "",
            1,
        )
        composed_doc.write_text(composed_doc_missing_run_id)
        missing_doc_run_id_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc_missing_run_id.replace(
                "Preflight artifacts:",
                "Run ID:\n`composed-1a2b`\n\nPreflight artifacts:",
                1,
            )
        )
        composed_doc_run_id_outside_method = (
            composed_doc.read_text()
            .replace(
                "Run ID:\n`composed-1a2b`\n\n",
                "",
                1,
            )
            .replace(
                "# Composed E2E\n\n",
                "# Composed E2E\n\nRun ID:\n`composed-1a2b`\n\n",
                1,
            )
        )
        composed_doc.write_text(composed_doc_run_id_outside_method)
        composed_doc_run_id_outside_method_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc_run_id_outside_method
            .replace(
                "# Composed E2E\n\nRun ID:\n`composed-1a2b`\n\n",
                "# Composed E2E\n\n",
                1,
            )
            .replace(
                "Preflight artifacts:",
                "Run ID:\n`composed-1a2b`\n\nPreflight artifacts:",
                1,
            )
        )
        composed_doc_identity_outside_method = (
            composed_doc.read_text()
            .replace(
                "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\n",
                "",
                1,
            )
            .replace(
                "# Composed E2E\n\n",
                "# Composed E2E\n\n"
                "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\n",
                1,
            )
        )
        composed_doc.write_text(composed_doc_identity_outside_method)
        composed_doc_identity_outside_method_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc_identity_outside_method
            .replace(
                "# Composed E2E\n\n"
                "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\n",
                "# Composed E2E\n\n",
                1,
            )
            .replace(
                "Preflight artifacts:",
                "Measured git commit:\n`cccccccccccccccccccccccccccccccccccccccc`\n\n"
                "Preflight artifacts:",
                1,
            )
        )
        composed_doc_preflight_outside_method = (
            composed_doc.read_text()
            .replace(
                "- kernel_image_sha256: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`\n",
                "",
                1,
            )
            .replace(
                "# Composed E2E\n\n",
                "# Composed E2E\n\n"
                "- kernel_image_sha256: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`\n\n",
                1,
            )
        )
        composed_doc.write_text(composed_doc_preflight_outside_method)
        composed_doc_preflight_outside_method_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc_preflight_outside_method
            .replace(
                "# Composed E2E\n\n"
                "- kernel_image_sha256: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`\n\n",
                "# Composed E2E\n\n",
                1,
            )
            .replace(
                "Preflight artifacts:\n\n",
                "Preflight artifacts:\n\n"
                "- kernel_image_sha256: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`\n",
                1,
            )
        )
        composed_doc_substrate_outside_method = (
            composed_doc.read_text()
            .replace("- host kernel release: `6.17.0-23-generic`\n", "", 1)
            .replace(
                "# Composed E2E\n\n",
                "# Composed E2E\n\n- host kernel release: `6.17.0-23-generic`\n\n",
                1,
            )
        )
        composed_doc.write_text(composed_doc_substrate_outside_method)
        composed_doc_substrate_outside_method_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc_substrate_outside_method
            .replace(
                "# Composed E2E\n\n- host kernel release: `6.17.0-23-generic`\n\n",
                "# Composed E2E\n\n",
                1,
            )
            .replace(
                "Substrate details:\n\n",
                "Substrate details:\n\n- host kernel release: `6.17.0-23-generic`\n",
                1,
            )
        )
        composed_doc_bad_host_memory_digest = composed_doc.read_text().replace(
            "Shared image digest: `1111111111111111111111111111111111111111111111111111111111111111`.\n\n",
            "",
            1,
        )
        composed_doc.write_text(composed_doc_bad_host_memory_digest)
        missing_doc_host_memory_digest_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc_bad_host_memory_digest.replace(
                "`after_n_attached_delta_bytes` stayed within bound.",
                "Shared image digest: `1111111111111111111111111111111111111111111111111111111111111111`.\n\n"
                "`after_n_attached_delta_bytes` stayed within bound.",
                1,
            )
        )
        composed_doc_bad_residue_root = composed_doc.read_text().replace(
            "- `/var/lib/m80-composed-e2e`",
            "- `/var/lib/m80-runs`",
        )
        composed_doc.write_text(composed_doc_bad_residue_root)
        missing_doc_residue_root_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc_bad_residue_root.replace(
                "- `/var/lib/m80-runs`",
                "- `/var/lib/m80-composed-e2e`",
            )
        )
        composed_doc_bad_restore_number = composed_doc.read_text().replace(
            "104.000 ms",
            "103.000 ms",
            1,
        )
        composed_doc.write_text(composed_doc_bad_restore_number)
        missing_doc_restore_number_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc_bad_restore_number.replace(
                "103.000 ms",
                "104.000 ms",
                1,
            )
        )
        composed_doc_bad_substrate = composed_doc.read_text().replace(
            "- host kernel release: `6.17.0-23-generic`",
            "- host kernel release: `6.1.0`",
        )
        composed_doc.write_text(composed_doc_bad_substrate)
        missing_doc_substrate_status = quiet_run_checks(args)
        composed_doc.write_text(
            composed_doc_bad_substrate.replace(
                "- host kernel release: `6.1.0`",
                "- host kernel release: `6.17.0-23-generic`",
            )
        )
        args.only = ["snapshot-template"]
        bad = json.loads(snapshot.read_text())
        bad["substrate"]["allow_other_firecracker_vms"] = True
        snapshot.write_text(json.dumps(bad))
        bad_status = quiet_run_checks(args)
        head_commit = git_stdout(["rev-parse", "HEAD"])
        head_short_commit = git_stdout(["rev-parse", "--short=7", "HEAD"])
        parent_commit = git_stdout(["rev-parse", "--verify", "HEAD^"])
        head_commit_for_tests = head_commit or "0" * 40
        head_short_commit_for_tests = head_short_commit or "0" * 7
        mismatch_commit = (
            parent_commit
            if parent_commit is not None and parent_commit != head_commit_for_tests
            else "0" * 40
        )
        valid_head_close_reason = (
            head_commit is not None
            and close_reason_commit(f"verified: README.md @ {head_commit}", "README.md") == head_commit
            and git_ok(["cat-file", "-e", f"{head_commit}^{{commit}}"])
            and git_ok(["cat-file", "-e", f"{head_commit}:README.md"])
            and git_ok(["diff", "--quiet", head_commit, "HEAD", "--", "README.md"])
        )
        short_full_commit_match = (
            head_commit is not None
            and head_short_commit is not None
            and same_commit(head_short_commit, head_commit)
        )
        invalid_repeated_commit_rejected = not same_commit("1111111", "1111111")
        composed_doc_expected_refs = [
            (
                "m80-q420k.6.2",
                "crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json",
                head_commit_for_tests,
            ),
            (
                "m80-q420k.6.3",
                "crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json",
                head_commit_for_tests,
            ),
            (
                "m80-q420k.6.4",
                "crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json",
                head_commit_for_tests,
            ),
        ]
        composed_doc_close_reason_valid = not verify_composed_doc_close_reason_refs(
            "verified: crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json "
            f"@ {head_short_commit_for_tests}\n"
            "verified: crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json "
            f"@ {head_short_commit_for_tests}\n"
            "verified: crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json "
            f"@ {head_short_commit_for_tests}\n"
            f"verified: docs/perf/composed-e2e.md @ {head_short_commit_for_tests}",
            composed_doc_expected_refs,
        )
        composed_doc_close_reason_bad = verify_composed_doc_close_reason_refs(
            f"verified: docs/perf/composed-e2e.md @ {head_commit_for_tests}",
            composed_doc_expected_refs,
        )
        composed_doc_close_reason_mismatch = verify_composed_doc_close_reason_refs(
            "verified: crates/m80-firecracker/benches/snapshots/composed-e2e-restore-N10.json "
            f"@ {mismatch_commit}\n"
            "verified: crates/m80-firecracker/benches/snapshots/composed-e2e-host-memory.json "
            f"@ {head_short_commit_for_tests}\n"
            "verified: crates/m80-firecracker/benches/snapshots/composed-e2e-residue.json "
            f"@ {head_short_commit_for_tests}",
            composed_doc_expected_refs,
        )
        super_epic_close_reason_valid = not verify_super_epic_close_reason_ref(
            f"verified: docs/perf/composed-e2e.md @ {head_short_commit_for_tests}",
            "m80-q420k.6",
            "docs/perf/composed-e2e.md",
            head_commit_for_tests,
        )
        super_epic_close_reason_bad = verify_super_epic_close_reason_ref(
            f"verified: docs/perf/composed-e2e.md @ {mismatch_commit}",
            "m80-q420k.6",
            "docs/perf/composed-e2e.md",
            head_commit_for_tests,
        )
        measured_commit_extraction = (
            artifact_measured_git_commit("snapshot-template", snapshot) == git_commit
            and artifact_measured_git_commit("pmem-density", density) == git_commit
            and artifact_measured_git_commit("composed-restore", restore) == git_commit
            and artifact_measured_git_commit("composed-doc", composed_doc) is None
        )
        density_commit_outside_substrate = (
            density.read_text()
            .replace("- git commit: `cccccccccccccccccccccccccccccccccccccccc`\n", "", 1)
            .replace(
                "# Shared pmem density\n\n",
                "# Shared pmem density\n\n"
                "- git commit: `cccccccccccccccccccccccccccccccccccccccc`\n\n",
                1,
            )
        )
        density.write_text(density_commit_outside_substrate)
        density_measured_commit_scoped = artifact_measured_git_commit("pmem-density", density) is None
        density.write_text(
            density_commit_outside_substrate
            .replace(
                "# Shared pmem density\n\n"
                "- git commit: `cccccccccccccccccccccccccccccccccccccccc`\n\n",
                "# Shared pmem density\n\n",
                1,
            )
            .replace(
                "- git worktree dirty excluding this artifact: `false`\n",
                "- git worktree dirty excluding this artifact: `false`\n"
                "- git commit: `cccccccccccccccccccccccccccccccccccccccc`\n",
                1,
            )
        )
        full_close_flags = argparse.Namespace(
            require_committed=True,
            require_closed_beads=True,
            require_parent_phases_closed=True,
            require_super_epic_closed=True,
        )
        missing_parent_support_flags = argparse.Namespace(
            require_committed=False,
            require_closed_beads=False,
            require_parent_phases_closed=True,
            require_super_epic_closed=False,
        )
        missing_super_support_flags = argparse.Namespace(
            require_committed=True,
            require_closed_beads=True,
            require_parent_phases_closed=False,
            require_super_epic_closed=True,
        )
        final_flag_composition = (
            not verify_flag_composition(full_close_flags, set())
            and verify_flag_composition(missing_parent_support_flags, set())
            and verify_flag_composition(missing_super_support_flags, set())
            and verify_flag_composition(full_close_flags, {"ext4-overlay"})
        )
        if (
            ok_status != 0
            or composed_doc_subset_status != 0
            or subset_parent_flag_status == 0
            or subset_super_flag_status == 0
            or close_artifact_paths_status != 0
            or close_artifact_paths_ignored_status == 0
            or ext4_status != 0
            or ext4_bad_status == 0
            or ext4_bad_command_status == 0
            or dax_status != 0
            or dax_bad_status == 0
            or dax_bad_reproduction_command_status == 0
            or dax_bad_digest_status == 0
            or dax_bad_delta_status == 0
            or dax_bad_substrate_status == 0
            or dax_bad_post_run_status == 0
            or dax_bad_preflight_status == 0
            or dax_bad_preflight_command_status == 0
            or bad_substrate_status == 0
            or leaked_firecracker_status == 0
            or bad_snapshot_host_kernel_status == 0
            or bad_snapshot_kvm_status == 0
            or bad_snapshot_sudo_status == 0
            or bad_snapshot_firecracker_version_status == 0
            or bad_preflight_artifacts_status == 0
            or stock_kernel_status == 0
            or bad_git_commit_status == 0
            or bad_snapshot_target_ready_status == 0
            or bad_snapshot_exact_shape_status == 0
            or bad_snapshot_sample_count_status == 0
            or bad_snapshot_sample_detail_status == 0
            or bad_snapshot_run_cycle_status == 0
            or bad_snapshot_runs_detail_status == 0
            or bad_snapshot_percentile_status == 0
            or bad_snapshot_allow_other_reproduction_command_status == 0
            or bad_snapshot_n_reproduction_command_status == 0
            or bad_snapshot_runs_reproduction_command_status == 0
            or bad_snapshot_reproduction_command_status == 0
            or bad_snapshot_jail_uid_reproduction_command_status == 0
            or snapshot_doc_bad_smoke_status == 0
            or snapshot_doc_duplicate_smoke_status == 0
            or snapshot_doc_bad_kernel_kind_status == 0
            or snapshot_doc_bad_command_identity_status == 0
            or snapshot_doc_bad_p99_scope_status == 0
            or snapshot_doc_bad_identity_status == 0
            or snapshot_doc_identity_outside_section_status == 0
            or snapshot_doc_preflight_outside_section_status == 0
            or snapshot_doc_runtime_outside_section_status == 0
            or density_bad_teardown_status == 0
            or density_bad_host_kernel_status == 0
            or density_bad_kvm_status == 0
            or density_bad_sudo_status == 0
            or density_bad_substrate_runtime_status == 0
            or density_bad_bound_status == 0
            or density_bad_digest_status == 0
            or density_bad_image_path_status == 0
            or density_bad_preflight_command_status == 0
            or density_duplicate_substrate_status == 0
            or density_bad_sample_status == 0
            or density_bad_exact_command_status == 0
            or density_bad_reproduction_command_status == 0
            or density_command_outside_reproduction_status == 0
            or density_observable_outside_section_status == 0
            or density_bad_trust_section_status == 0
            or density_smoke_status != 0
            or density_smoke_bad_repro_command_status == 0
            or density_smoke_commented_guard_status == 0
            or density_smoke_not_executable_status == 0
            or density_runbook_status != 0
            or density_runbook_bad_allow_other_status == 0
            or density_runbook_bad_kernel_kind_status == 0
            or density_runbook_bad_prepared_sha_status == 0
            or density_runbook_bad_firecracker_sha_status == 0
            or density_runbook_outside_command_status == 0
            or quiet_host_inventory_status != 0
            or quiet_host_inventory_not_executable_status == 0
            or quiet_host_inventory_mutating_status == 0
            or restore_bad_page_cache_status == 0
            or restore_bad_run_id_status == 0
            or restore_bad_host_kernel_status == 0
            or restore_bad_kvm_status == 0
            or restore_bad_sudo_status == 0
            or restore_bad_firecracker_version_status == 0
            or restore_bad_target_ready_status == 0
            or restore_bad_exact_n_status == 0
            or restore_bad_sample_count_status == 0
            or restore_bad_percentile_status == 0
            or restore_bad_warm_pool_status == 0
            or restore_bad_diagnostics_phase_status == 0
            or restore_bad_exec_count_status == 0
            or restore_bad_pmem_layers_status == 0
            or memory_bad_bound_status == 0
            or memory_bad_n_status == 0
            or memory_bad_digest_status == 0
            or memory_bad_delta_status == 0
            or memory_bad_path_status == 0
            or memory_bad_per_vm_baseline_status == 0
            or memory_bad_per_vm_baseline_overhead_status == 0
            or residue_bad_digest_status == 0
            or residue_bad_image_expected_status == 0
            or residue_bad_template_fingerprint_status == 0
            or residue_bad_exact_n_status == 0
            or residue_bad_roots_status == 0
            or residue_bad_run_root_status == 0
            or residue_bad_leased_run_dir_status == 0
            or residue_bad_leased_run_dir_root_status == 0
            or composed_bad_consistency_status == 0
            or composed_bad_run_id_consistency_status == 0
            or uncommitted_status == 0
            or diagnostic_doc_status == 0
            or composed_doc_bad_artifact_marker_status == 0
            or composed_doc_smoke_outside_section_status == 0
            or missing_doc_command_status == 0
            or composed_doc_bad_helper_status == 0
            or composed_doc_bad_run_root_status == 0
            or composed_doc_bad_kernel_kind_status == 0
            or composed_playbook_status != 0
            or composed_playbook_bad_helper_status == 0
            or composed_playbook_bad_run_root_status == 0
            or composed_playbook_artifact_outside_section_status == 0
            or missing_doc_json_identity_status == 0
            or missing_doc_run_id_status == 0
            or composed_doc_run_id_outside_method_status == 0
            or composed_doc_identity_outside_method_status == 0
            or composed_doc_preflight_outside_method_status == 0
            or composed_doc_substrate_outside_method_status == 0
            or missing_doc_host_memory_digest_status == 0
            or missing_doc_residue_root_status == 0
            or missing_doc_restore_number_status == 0
            or missing_doc_substrate_status == 0
            or bad_status == 0
            or not valid_head_close_reason
            or not short_full_commit_match
            or not invalid_repeated_commit_rejected
            or not composed_doc_close_reason_valid
            or not composed_doc_close_reason_bad
            or not composed_doc_close_reason_mismatch
            or not super_epic_close_reason_valid
            or not super_epic_close_reason_bad
            or not measured_commit_extraction
            or not density_measured_commit_scoped
            or not final_flag_composition
            or not close_reason_matches(
                "verified: docs/perf/pmem-shared-density.md @ 0123abc",
                "docs/perf/pmem-shared-density.md",
            )
            or close_reason_matches(
                "verified: docs/perf/pmem-shared-density.md",
                "docs/perf/pmem-shared-density.md",
            )
        ):
            print("self-test failed", file=sys.stderr)
            return 1
    print("self-test ok")
    return 0


def quiet_run_checks(args: argparse.Namespace) -> int:
    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        return run_checks(args)


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--snapshot-template", default=DEFAULT_SNAPSHOT)
    p.add_argument("--snapshot-doc", default=DEFAULT_SNAPSHOT_DOC)
    p.add_argument("--pmem-density", default=DEFAULT_DENSITY)
    p.add_argument("--pmem-density-smoke", default=DEFAULT_DENSITY_SMOKE)
    p.add_argument("--close-runbook", default=DEFAULT_CLOSE_RUNBOOK)
    p.add_argument("--measurement-playbook", default=DEFAULT_MEASUREMENT_PLAYBOOK)
    p.add_argument("--quiet-host-inventory", default=DEFAULT_QUIET_HOST_INVENTORY)
    p.add_argument("--composed-restore", default=DEFAULT_COMPOSED_RESTORE)
    p.add_argument("--composed-memory", default=DEFAULT_COMPOSED_MEMORY)
    p.add_argument("--composed-residue", default=DEFAULT_COMPOSED_RESIDUE)
    p.add_argument("--composed-doc", default=DEFAULT_COMPOSED_DOC)
    p.add_argument("--ext4-overlay", default=DEFAULT_EXT4_OVERLAY)
    p.add_argument("--dax-memory-pressure", default=DEFAULT_DAX_MEMORY_PRESSURE)
    p.add_argument(
        "--only",
        action="append",
        choices=[
            "snapshot-template",
            "snapshot-doc",
            "close-artifact-paths",
            "quiet-host-inventory",
            "prepared-inputs",
            "pmem-density",
            "pmem-density-smoke",
            "pmem-density-runbook",
            "pmem-density-playbook",
            "composed-restore",
            "composed-memory",
            "composed-residue",
            "composed-doc",
            "composed-playbook",
            "ext4-overlay",
            "dax-memory-pressure",
        ],
        help="Run only one named check. Repeat to run a subset; default is all checks.",
    )
    p.add_argument("--self-test", action="store_true")
    p.add_argument(
        "--require-committed",
        action="store_true",
        help="Also require each selected artifact/doc/script path to exist in HEAD with no staged or unstaged changes.",
    )
    p.add_argument(
        "--require-closed-beads",
        action="store_true",
        help="Also require each selected measurement bead to be closed with a verified artifact close reason.",
    )
    p.add_argument(
        "--require-parent-phases-closed",
        action="store_true",
        help="Also require Phase 0 and Phases A-F under m80-q420k to be closed.",
    )
    p.add_argument(
        "--require-super-epic-closed",
        action="store_true",
        help="Also require m80-q420k itself to be closed with the Phase F receipt evidence.",
    )
    return p


def main() -> int:
    args = parser().parse_args()
    if args.self_test:
        return run_self_tests()
    return run_checks(args)


if __name__ == "__main__":
    raise SystemExit(main())
