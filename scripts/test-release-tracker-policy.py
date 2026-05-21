#!/usr/bin/env python3
"""Tests for scripts/verify-release-tracker-policy.py."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
VERIFY = REPO_ROOT / "scripts" / "verify-release-tracker-policy.py"


class ReleaseTrackerPolicyTest(unittest.TestCase):
    def test_missing_verified_close_label_fails(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.18",
                        "Privileged release smoke runner",
                        labels=["cicd", "release", "requires-verified-close"],
                        parent="m80-o3uh9",
                    ),
                    issue(
                        "m80-o3uh9.18.2",
                        "Real-KVM runner baseline",
                        "proof artifact records runner identity and substrate summary",
                        labels=["cicd", "real-kvm", "release"],
                        parent="m80-o3uh9.18",
                    ),
                ],
            )

            result = repo.run_verify()

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "m80-o3uh9.18.2: must carry requires-verified-close",
                result.stderr,
            )

    def test_parent_inherits_missing_verified_close_label(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.17",
                        "Hostless release fixture CI",
                        labels=["cicd", "quickstart", "release"],
                        parent="m80-o3uh9",
                    ),
                    issue(
                        "m80-o3uh9.17.1",
                        "Fake release server fixture",
                        "fixture emits a hostless proof artifact",
                        labels=["cicd", "quickstart", "release", "requires-verified-close"],
                        parent="m80-o3uh9.17",
                    ),
                ],
            )

            result = repo.run_verify()

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "m80-o3uh9.17: must carry requires-verified-close",
                result.stderr,
            )

    def test_missing_verified_close_reason_fails(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.18.2",
                        "Real-KVM runner baseline",
                        status="closed",
                        labels=["cicd", "real-kvm", "release", "requires-verified-close"],
                        parent="m80-o3uh9",
                    ),
                ],
            )

            result = repo.run_verify()

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("closed requires-verified-close issue missing close_reason", result.stderr)

    def test_new_closed_proof_leaf_without_label_fails(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.18.2",
                        "Real-KVM runner baseline",
                        "proof artifact records runner identity and substrate summary",
                        status="closed",
                        labels=["cicd", "real-kvm", "release"],
                        parent="m80-o3uh9",
                        closed_at="2026-05-20T18:30:00+00:00",
                    ),
                ],
            )

            result = repo.run_verify()

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "m80-o3uh9.18.2: must carry requires-verified-close",
                result.stderr,
            )

    def test_stale_artifact_path_fails(self) -> None:
        with tracker_repo() as repo:
            sha = repo.commit_all("issues only")
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.18.2",
                        "Real-KVM runner baseline",
                        status="closed",
                        close_reason=f"verified: artifacts/missing.json @ {sha}",
                        labels=["cicd", "real-kvm", "release", "requires-verified-close"],
                        parent="m80-o3uh9",
                    ),
                ],
            )

            result = repo.run_verify()

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("verified artifact path is missing: artifacts/missing.json", result.stderr)

    def test_valid_close_note_passes(self) -> None:
        with tracker_repo() as repo:
            proof = repo.root / "artifacts" / "proof.json"
            proof.parent.mkdir()
            proof.write_text(json.dumps(valid_quickstart_proof()))
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.18.2",
                        "Real-KVM runner baseline",
                        status="closed",
                        labels=["cicd", "real-kvm", "release", "requires-verified-close"],
                        parent="m80-o3uh9",
                    ),
                ],
            )
            sha = repo.commit_all("valid proof")
            issues = read_issues(repo.root)
            issues[1]["close_reason"] = f"verified: artifacts/proof.json @ {sha}"
            write_issues(repo.root, issues)

            result = repo.run_verify()

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("release tracker policy ok", result.stdout)

    def test_valid_close_matrix_artifact_passes_schema_check(self) -> None:
        with tracker_repo() as repo:
            result = repo.run_with_close_matrix(valid_close_matrix())

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_close_matrix_missing_top_level_field_fails(self) -> None:
        with tracker_repo() as repo:
            matrix = valid_close_matrix()
            del matrix["tracker_digest"]

            result = repo.run_with_close_matrix(matrix)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("tracker_digest must be sha256:<64 lowercase hex>", result.stderr)

    def test_close_matrix_malformed_row_fails(self) -> None:
        with tracker_repo() as repo:
            matrix = valid_close_matrix()
            matrix["rows"][0]["id"] = ""

            result = repo.run_with_close_matrix(matrix)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("rows[0]: id must be a nonempty string", result.stderr)

    def test_close_matrix_escaping_proof_artifact_path_fails(self) -> None:
        with tracker_repo() as repo:
            matrix = valid_close_matrix()
            matrix["rows"][0]["proof_artifact"] = "../proof.json"

            result = repo.run_with_close_matrix(matrix)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("proof_artifact must be a relative path", result.stderr)

    def test_close_matrix_unknown_schema_version_fails(self) -> None:
        with tracker_repo() as repo:
            matrix = valid_close_matrix()
            matrix["schema_version"] = 2

            result = repo.run_with_close_matrix(matrix)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("schema_version must be 1", result.stderr)

    def test_no_arg_quickstart_public_selector_fails(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.4",
                        "Selector",
                        "Public common path runs `m80 quickstart` to select latest assets.",
                        labels=["quickstart", "release"],
                        parent="m80-o3uh9",
                    ),
                ],
            )

            result = repo.run_verify()

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("m80-o3uh9.4: description contains unsupported", result.stderr)
            self.assertIn("no-arg m80 quickstart is not the public release selector", result.stderr)
            self.assertIn("expected use latest/pinned release install.sh", result.stderr)

    def test_explicit_quickstart_override_passes(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.4.2",
                        "Quickstart explicit artifact override verifier",
                        "`m80 quickstart --artifact-url <url>` remains a local fixture/operator override.",
                        labels=["quickstart", "release"],
                        parent="m80-o3uh9",
                    ),
                ],
            )

            result = repo.run_verify()

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_public_install_sh_claim_passes(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        "Public path: curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                ],
            )

            result = repo.run_verify()

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_raw_main_installer_url_fails(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.12",
                        "Docs",
                        "Install with https://raw.githubusercontent.com/moradology/m80/main/scripts/install.sh",
                        labels=["docs", "release"],
                        parent="m80-o3uh9",
                    ),
                ],
            )

            result = repo.run_verify()

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("raw main installer URLs are mutable", result.stderr)

    def test_artifact_only_latest_url_fails(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.19",
                        "Legacy quickstart",
                        "curl https://github.com/moradology/m80/releases/latest/download/m80-linux-x86_64-minimal-artifacts.tar.gz",
                        labels=["quickstart", "release"],
                        parent="m80-o3uh9",
                    ),
                ],
            )

            result = repo.run_verify()

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("artifact-only releases/latest URLs are not the public install selector", result.stderr)

    def test_wrong_owner_public_download_fails(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.12",
                        "Docs",
                        "Use https://github.com/someone-else/m80/releases/latest/download/install.sh",
                        labels=["docs", "release"],
                        parent="m80-o3uh9",
                    ),
                ],
            )

            result = repo.run_verify()

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("public m80 release downloads must use moradology/m80", result.stderr)

    def test_acceptance_criteria_and_close_reason_are_scanned(self) -> None:
        with tracker_repo() as repo:
            docs_issue = issue(
                "m80-o3uh9.12",
                "Docs",
                labels=["docs", "release"],
                parent="m80-o3uh9",
            )
            docs_issue["acceptance_criteria"] = (
                "Common path uses https://raw.githubusercontent.com/moradology/m80/main/scripts/install.sh"
            )
            closed_issue = issue(
                "m80-o3uh9.19",
                "Legacy quickstart",
                status="closed",
                labels=["quickstart", "release"],
                parent="m80-o3uh9",
                close_reason=(
                    "Public quickstart repaired with "
                    "https://github.com/example/m80/releases/latest/download/install.sh"
                ),
            )
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    docs_issue,
                    closed_issue,
                ],
            )

            result = repo.run_verify()

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("m80-o3uh9.12: acceptance_criteria contains unsupported", result.stderr)
            self.assertIn("raw main installer URLs are mutable", result.stderr)
            self.assertIn("m80-o3uh9.19: close_reason contains unsupported", result.stderr)
            self.assertIn("public m80 release downloads must use moradology/m80", result.stderr)

    def test_policy_config_non_default_epoch_passes(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "release-next",
                        "Next release epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "release-next.1",
                        "Install docs",
                        "Public path uses latest install.sh.",
                        labels=["docs", "release"],
                        parent="release-next",
                    ),
                ],
            )
            config = write_policy_config(
                repo.root,
                [{"id": "release-next", "status": "active"}],
            )

            result = repo.run_verify("--policy-config", str(config))

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_policy_config_multiple_active_epochs_pass(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "release-next",
                        "Next release epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "release-next.1",
                        "Fixture quickstart",
                        "`m80 quickstart --artifact-url <url>` remains a test override.",
                        labels=["quickstart", "release"],
                        parent="release-next",
                    ),
                ],
            )
            config = write_policy_config(
                repo.root,
                [
                    {"id": "m80-o3uh9", "status": "active"},
                    {"id": "release-next", "status": "active"},
                ],
            )

            result = repo.run_verify("--policy-config", str(config))

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_policy_config_second_active_epoch_failure_names_config_and_epoch(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "release-next",
                        "Next release epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "release-next.1",
                        "Stale quickstart docs",
                        "Public latest selector uses `m80 quickstart`.",
                        labels=["quickstart", "release"],
                        parent="release-next",
                    ),
                ],
            )
            config = write_policy_config(
                repo.root,
                [
                    {"id": "m80-o3uh9", "status": "active"},
                    {"id": "release-next", "status": "active"},
                ],
            )

            result = repo.run_verify("--policy-config", str(config))

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(str(config), result.stderr)
            self.assertIn("active epoch release-next", result.stderr)
            self.assertIn("release-next.1: description contains unsupported", result.stderr)

    def test_policy_config_missing_active_epoch_fails(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    )
                ],
            )
            config = write_policy_config(
                repo.root,
                [{"id": "missing-release", "status": "active"}],
            )

            result = repo.run_verify("--policy-config", str(config))

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("active epoch missing-release is missing from tracker", result.stderr)

    def test_policy_config_retired_epoch_with_reason_passes(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    )
                ],
            )
            config = write_policy_config(
                repo.root,
                [
                    {"id": "m80-o3uh9", "status": "active"},
                    {
                        "id": "release-old",
                        "status": "retired",
                        "reason": "superseded by m80-o3uh9 after release proof landed",
                    },
                ],
            )

            result = repo.run_verify("--policy-config", str(config))

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_policy_config_retired_epoch_without_reason_fails(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["quickstart", "release", "requires-verified-close"],
                    )
                ],
            )
            config = write_policy_config(
                repo.root,
                [
                    {"id": "m80-o3uh9", "status": "active"},
                    {"id": "release-old", "status": "retired"},
                ],
            )

            result = repo.run_verify("--policy-config", str(config))

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("retired epochs require a nonempty reason", result.stderr)

    def test_policy_config_omitted_active_root_epoch_fails(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["epoch", "quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "release-next",
                        "Next release epoch",
                        labels=["epoch", "quickstart", "release", "requires-verified-close"],
                    ),
                ],
            )
            config = write_policy_config(
                repo.root,
                [{"id": "release-next", "status": "active"}],
            )

            result = repo.run_verify("--policy-config", str(config))

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("active tracker epoch m80-o3uh9 is missing from policy config", result.stderr)

    def test_policy_config_omitted_future_active_epoch_fails(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["epoch", "quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "release-future",
                        "Future release epoch",
                        labels=["epoch", "release", "requires-verified-close"],
                    ),
                ],
            )
            config = write_policy_config(
                repo.root,
                [{"id": "m80-o3uh9", "status": "active"}],
            )

            result = repo.run_verify("--policy-config", str(config))

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "active tracker epoch release-future is missing from policy config",
                result.stderr,
            )

    def test_policy_config_release_child_is_not_epoch_candidate(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["epoch", "quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "m80-o3uh9.13",
                        "Release CI hardening",
                        labels=["cicd", "release"],
                        parent="m80-o3uh9",
                    ),
                ],
            )
            config = write_policy_config(
                repo.root,
                [{"id": "m80-o3uh9", "status": "active"}],
            )

            result = repo.run_verify("--policy-config", str(config))

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_policy_config_open_epoch_cannot_be_retired(self) -> None:
        with tracker_repo() as repo:
            write_issues(
                repo.root,
                [
                    issue(
                        "m80-o3uh9",
                        "Epoch",
                        labels=["epoch", "quickstart", "release", "requires-verified-close"],
                    ),
                    issue(
                        "release-old",
                        "Old release epoch",
                        labels=["epoch", "release", "requires-verified-close"],
                    ),
                ],
            )
            config = write_policy_config(
                repo.root,
                [
                    {"id": "m80-o3uh9", "status": "active"},
                    {
                        "id": "release-old",
                        "status": "retired",
                        "reason": "superseded by m80-o3uh9",
                    },
                ],
            )

            result = repo.run_verify("--policy-config", str(config))

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("active tracker epoch release-old is configured as retired", result.stderr)

    def test_closed_configured_epoch_without_close_reason_fails(self) -> None:
        with tracker_repo() as repo:
            result = repo.run_with_closed_epoch(close_reason=None)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "closed release epoch requires final close-matrix close_reason",
                result.stderr,
            )

    def test_closed_configured_epoch_missing_matrix_artifact_fails(self) -> None:
        with tracker_repo() as repo:
            placeholder = repo.root / "placeholder.txt"
            placeholder.write_text("placeholder")
            sha = repo.commit_all("placeholder")

            result = repo.run_with_closed_epoch(
                close_reason=f"verified: artifacts/missing-matrix.json @ {sha}",
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("final close matrix path is missing", result.stderr)

    def test_closed_configured_epoch_non_matrix_artifact_fails(self) -> None:
        with tracker_repo() as repo:
            result = repo.run_with_closed_epoch(artifact=valid_quickstart_proof())

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                "final close matrix artifact must have kind release_final_close_matrix",
                result.stderr,
            )

    def test_closed_configured_epoch_with_valid_matrix_passes(self) -> None:
        with tracker_repo() as repo:
            result = repo.run_with_closed_epoch(artifact=valid_close_matrix())

            self.assertEqual(result.returncode, 0, result.stderr)

    def test_close_matrix_omitted_descendant_fails(self) -> None:
        with tracker_repo() as repo:
            descendants = [
                matrix_child("m80-o3uh9.1"),
                matrix_child("m80-o3uh9.2"),
            ]

            result = repo.run_with_closed_epoch(
                artifact=valid_close_matrix(),
                descendants=descendants,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("matrix omits descendant m80-o3uh9.2", result.stderr)

    def test_close_matrix_unknown_descendant_row_fails(self) -> None:
        with tracker_repo() as repo:
            matrix = valid_close_matrix()
            matrix["rows"][0]["id"] = "m80-o3uh9.99"

            result = repo.run_with_closed_epoch(artifact=matrix)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("matrix row references unknown descendant m80-o3uh9.99", result.stderr)

    def test_close_matrix_stale_status_fails(self) -> None:
        with tracker_repo() as repo:
            descendants = [matrix_child("m80-o3uh9.1", status="open")]

            result = repo.run_with_closed_epoch(
                artifact=valid_close_matrix(),
                descendants=descendants,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("m80-o3uh9.1 status is stale", result.stderr)

    def test_close_matrix_open_descendant_fails(self) -> None:
        with tracker_repo() as repo:
            matrix = valid_close_matrix()
            matrix["rows"][0]["status"] = "open"
            descendants = [matrix_child("m80-o3uh9.1", status="open")]

            result = repo.run_with_closed_epoch(
                artifact=matrix,
                descendants=descendants,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("m80-o3uh9.1 descendant remains open with status open", result.stderr)

    def test_close_matrix_proof_shaped_flag_mismatch_fails(self) -> None:
        with tracker_repo() as repo:
            matrix = valid_close_matrix()
            matrix["rows"][0]["requires_verified_close"] = False
            descendants = [
                matrix_child(
                    "m80-o3uh9.1",
                    title="Real-KVM runner baseline",
                    labels=["real-kvm", "release"],
                )
            ]

            result = repo.run_with_closed_epoch(
                artifact=matrix,
                descendants=descendants,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("m80-o3uh9.1 requires_verified_close must be true", result.stderr)


class tracker_repo:
    def __enter__(self) -> "tracker_repo":
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / ".beads").mkdir()
        (self.root / ".beads" / "issues.jsonl").write_text("")
        subprocess.run(["git", "init"], cwd=self.root, check=True, stdout=subprocess.PIPE)
        subprocess.run(["git", "config", "user.email", "tester@example.com"], cwd=self.root, check=True)
        subprocess.run(["git", "config", "user.name", "tester"], cwd=self.root, check=True)
        return self

    def __exit__(self, *_exc: object) -> None:
        self.tmp.cleanup()

    def run_verify(self, *extra_args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                "python3",
                str(VERIFY),
                "--issues",
                str(self.root / ".beads" / "issues.jsonl"),
                "--repo-root",
                str(self.root),
                *extra_args,
            ],
            cwd=REPO_ROOT,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )

    def commit_all(self, message: str) -> str:
        subprocess.run(["git", "add", "."], cwd=self.root, check=True)
        subprocess.run(
            ["git", "commit", "-m", message],
            cwd=self.root,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.root, text=True).strip()

    def run_with_close_matrix(self, matrix: dict) -> subprocess.CompletedProcess[str]:
        matrix_path = self.root / "artifacts" / "close-matrix.json"
        matrix_path.parent.mkdir()
        matrix_path.write_text(json.dumps(matrix, indent=2, sort_keys=True))
        sha = self.commit_all("close matrix")
        write_issues(
            self.root,
            [
                issue(
                    "m80-o3uh9",
                    "Epoch",
                    labels=["quickstart", "release", "requires-verified-close"],
                ),
                issue(
                    "m80-o3uh9.1",
                    "Close matrix leaf",
                    status="closed",
                    close_reason=f"verified: artifacts/close-matrix.json @ {sha}",
                    labels=["release", "requires-verified-close"],
                    parent="m80-o3uh9",
                ),
            ],
        )
        return self.run_verify()

    def run_with_closed_epoch(
        self,
        *,
        artifact: dict | None = None,
        close_reason: str | None = "",
        descendants: list[dict] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        config = write_policy_config(
            self.root,
            [{"id": "m80-o3uh9", "status": "active"}],
        )
        if artifact is not None:
            artifact_path = self.root / "artifacts" / "close-matrix.json"
            artifact_path.parent.mkdir()
            artifact_path.write_text(json.dumps(artifact, indent=2, sort_keys=True))
            sha = self.commit_all("closed epoch artifact")
            close_reason = f"verified: artifacts/close-matrix.json @ {sha}"
        closed_epoch = issue(
            "m80-o3uh9",
            "Epoch",
            status="closed",
            labels=["epoch", "quickstart", "release", "requires-verified-close"],
            close_reason=close_reason,
            closed_at="2026-05-21T00:00:00+00:00",
        )
        if close_reason is None:
            closed_epoch.pop("close_reason", None)
        write_issues(self.root, [closed_epoch, *(descendants or [matrix_child("m80-o3uh9.1")])])
        return self.run_verify("--policy-config", str(config))


def issue(
    issue_id: str,
    title: str,
    description: str = "",
    *,
    status: str = "open",
    labels: list[str] | None = None,
    parent: str | None = None,
    close_reason: str | None = None,
    closed_at: str | None = None,
) -> dict:
    value: dict[str, object] = {
        "id": issue_id,
        "title": title,
        "description": description,
        "status": status,
        "priority": 1,
        "issue_type": "task",
        "labels": labels or [],
        "dependencies": [],
    }
    if parent:
        value["dependencies"] = [
            {
                "issue_id": issue_id,
                "depends_on_id": parent,
                "type": "parent-child",
            }
        ]
    if close_reason is not None:
        value["close_reason"] = close_reason
    if closed_at is not None:
        value["closed_at"] = closed_at
        value["updated_at"] = closed_at
    return value


def matrix_child(
    issue_id: str,
    *,
    title: str = "Matrix child",
    status: str = "closed",
    labels: list[str] | None = None,
) -> dict:
    return issue(
        issue_id,
        title,
        status=status,
        labels=labels or ["release"],
        parent="m80-o3uh9",
    )


def write_issues(root: Path, issues: list[dict]) -> None:
    with (root / ".beads" / "issues.jsonl").open("w") as f:
        for issue_row in issues:
            f.write(json.dumps(issue_row, sort_keys=True))
            f.write("\n")


def write_policy_config(root: Path, epochs: list[dict]) -> Path:
    path = root / "release-tracker-policy.json"
    path.write_text(json.dumps({"schema_version": 1, "epochs": epochs}, indent=2))
    return path


def read_issues(root: Path) -> list[dict]:
    with (root / ".beads" / "issues.jsonl").open() as f:
        return [json.loads(line) for line in f if line.strip()]


def valid_quickstart_proof() -> dict:
    return {
        "release": {"resolved_tag": "v0.0.0"},
        "command": {"display": "m80 run -- echo hello", "observed_exit_status": 0},
        "stdout": {"excerpt": "hello\n"},
        "stderr": {"excerpt": ""},
        "substrate": {"kind": "real-kvm", "summary": "test runner"},
    }


def valid_close_matrix() -> dict:
    return {
        "schema_version": 1,
        "kind": "release_final_close_matrix",
        "epoch_id": "m80-o3uh9",
        "tracker_digest": f"sha256:{'a' * 64}",
        "generated_at": "2026-05-21T00:00:00Z",
        "rows": [
            {
                "id": "m80-o3uh9.1",
                "status": "closed",
                "behavior_doc": "docs/behaviors/release/verified-close-policy.md",
                "test_command": "python3 scripts/test-release-tracker-policy.py",
                "requires_verified_close": True,
                "requires_real_substrate": False,
            }
        ],
    }


if __name__ == "__main__":
    unittest.main()
