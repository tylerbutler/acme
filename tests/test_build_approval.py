import copy
import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parent.parent / "scripts/check-build-approval.py"
SPEC = importlib.util.spec_from_file_location("build_approval", SCRIPT)
approval = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(approval)

REPOSITORY = "owner/acme"
SPEC_PATH = "docs/specs/0010-coordinator.md"
BODY = (
    "## Spec and approval\n"
    "- [Spec PR: owner/acme#44](https://github.com/owner/acme/pull/44).\n"
    "- [Contract](https://github.com/owner/acme/blob/main/"
    "docs/specs/0010-coordinator.md).\n"
)


class ApprovalTests(unittest.TestCase):
    def setUp(self):
        self.responses = {
            "repos/owner/acme/issues/42": {
                "number": 42,
                "state": "open",
                "body": BODY,
            },
            "repos/owner/acme/contents/docs/specs/0010-coordinator.md?ref=main": {
                "type": "file",
                "path": SPEC_PATH,
                "sha": "a" * 40,
            },
            "repos/owner/acme/pulls/44": {
                "number": 44,
                "state": "closed",
                "merged": True,
                "base": {"ref": "main", "repo": {"full_name": REPOSITORY}},
            },
            "repos/owner/acme/pulls/44/files?per_page=100&page=1": [
                {"filename": SPEC_PATH, "status": "added"}
            ],
        }

    def api(self, endpoint):
        result = self.responses[endpoint]
        if isinstance(result, Exception):
            raise result
        return copy.deepcopy(result)

    def check(self):
        return approval.check_approval(REPOSITORY, 42, "main", self.api)

    def test_accepts_merged_pr_even_when_its_state_is_closed(self):
        result = self.check()
        self.assertEqual(result["specs"], [SPEC_PATH])
        self.assertEqual(result["spec_pull_requests"], [44])

    def test_rejects_unmerged_and_unknown_merge_evidence(self):
        for metadata in (
            {"state": "open", "merged": False},
            {"state": "closed", "merged": False},
            {"state": "closed", "merged": None},
            {"state": "closed"},
            {"state": "closed", "merged": "true"},
        ):
            with self.subTest(metadata=metadata):
                pull = self.responses["repos/owner/acme/pulls/44"]
                pull.pop("merged", None)
                pull.update(metadata)
                with self.assertRaisesRegex(ValueError, "merge"):
                    self.check()

    def test_preserves_main_only_spec_links(self):
        self.responses["repos/owner/acme/issues/42"]["body"] = (
            "[Bootstrap spec](https://github.com/owner/acme/blob/main/"
            "docs/specs/0010-coordinator.md)"
        )
        del self.responses["repos/owner/acme/pulls/44"]
        result = self.check()
        self.assertEqual(result["specs"], [SPEC_PATH])
        self.assertEqual(result["spec_pull_requests"], [])

    def test_checks_draft_revision_links_against_main(self):
        self.responses["repos/owner/acme/issues/42"]["body"] = BODY.replace(
            "/blob/main/", "/blob/237a51670c7a5fbe369f380aa132a6f9921984fa/"
        )
        self.assertEqual(self.check()["specs"], [SPEC_PATH])

    def test_rejects_wrong_repository_or_base_branch(self):
        for base in (
            {"ref": "feature", "repo": {"full_name": REPOSITORY}},
            {"ref": "main", "repo": {"full_name": "other/acme"}},
            {},
        ):
            with self.subTest(base=base):
                self.responses["repos/owner/acme/pulls/44"]["base"] = base
                with self.assertRaisesRegex(ValueError, "base"):
                    self.check()

    def test_requires_the_spec_pr_to_change_the_linked_spec(self):
        for files in (
            [{"filename": "README.md", "status": "modified"}],
            [{"filename": SPEC_PATH, "status": "removed"}],
            [],
        ):
            with self.subTest(files=files):
                self.responses[
                    "repos/owner/acme/pulls/44/files?per_page=100&page=1"
                ] = files
                with self.assertRaisesRegex(ValueError, "linked spec"):
                    self.check()

    def test_reads_all_pages_of_spec_pr_files(self):
        self.responses["repos/owner/acme/pulls/44/files?per_page=100&page=1"] = [
            {"filename": f"src/file-{number}.rs", "status": "modified"}
            for number in range(100)
        ]
        self.responses["repos/owner/acme/pulls/44/files?per_page=100&page=2"] = [
            {"filename": SPEC_PATH, "status": "modified"}
        ]
        self.assertEqual(self.check()["spec_pull_requests"], [44])

    def test_rejects_missing_or_unreadable_evidence(self):
        for endpoint in (
            "repos/owner/acme/issues/42",
            "repos/owner/acme/contents/docs/specs/0010-coordinator.md?ref=main",
            "repos/owner/acme/pulls/44",
        ):
            with self.subTest(endpoint=endpoint):
                original = self.responses[endpoint]
                self.responses[endpoint] = ValueError("GitHub API unavailable")
                with self.assertRaisesRegex(ValueError, "unavailable"):
                    self.check()
                self.responses[endpoint] = original

    def test_rejects_foreign_missing_or_unsafe_spec_links(self):
        for body in (
            "",
            BODY.replace("/owner/acme/", "/other/acme/"),
            BODY.replace("docs/specs/0010-coordinator.md", "README.md"),
            BODY.replace(
                "docs/specs/0010-coordinator.md", "docs/specs/../coordinator.md"
            ),
        ):
            with self.subTest(body=body):
                self.responses["repos/owner/acme/issues/42"]["body"] = body
                with self.assertRaises(ValueError):
                    self.check()

    def test_rejects_foreign_explicit_spec_pr(self):
        self.responses["repos/owner/acme/issues/42"]["body"] = BODY.replace(
            "github.com/owner/acme/pull/44", "github.com/other/acme/pull/44"
        )
        with self.assertRaisesRegex(ValueError, "repository"):
            self.check()

    def test_does_not_treat_an_unrelated_pr_as_spec_approval(self):
        self.responses["repos/owner/acme/issues/42"]["body"] = (
            BODY + "\nPrevious implementation: https://github.com/owner/acme/pull/48\n"
        )
        self.assertEqual(self.check()["spec_pull_requests"], [44])

    def test_does_not_treat_prose_about_spec_prs_as_an_approval_link(self):
        self.responses["repos/owner/acme/issues/42"]["body"] = BODY + (
            "\nThe old run thought the spec PR was closed without merging.\n"
            "Check spec PR merge metadata, not the closed state.\n"
        )
        self.assertEqual(self.check()["spec_pull_requests"], [44])

    def test_rejects_an_explicit_spec_pr_label_without_a_valid_link(self):
        self.responses["repos/owner/acme/issues/42"]["body"] = BODY.replace(
            "https://github.com/owner/acme/pull/44", "not-a-pull-request"
        )
        with self.assertRaisesRegex(ValueError, "explicit spec PR"):
            self.check()

    def test_rejects_malformed_base_metadata_with_a_useful_reason(self):
        for base in ("main", {"ref": "main", "repo": "owner/acme"}):
            with self.subTest(base=base):
                self.responses["repos/owner/acme/pulls/44"]["base"] = base
                with self.assertRaisesRegex(ValueError, "base"):
                    self.check()

    def test_rejects_directory_or_wrong_path_instead_of_spec_file(self):
        endpoint = "repos/owner/acme/contents/docs/specs/0010-coordinator.md?ref=main"
        for value in (
            {"type": "dir", "path": SPEC_PATH},
            {"type": "file", "path": "README.md"},
            None,
        ):
            with self.subTest(value=value):
                self.responses[endpoint] = value
                with self.assertRaisesRegex(ValueError, "spec"):
                    self.check()

    def test_cli_emits_approval_only_after_success(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            summary = Path(directory) / "summary"
            env = {
                "GITHUB_REPOSITORY": REPOSITORY,
                "ISSUE_NUMBER": "42",
                "DEFAULT_BRANCH": "main",
                "GITHUB_OUTPUT": str(output),
                "GITHUB_STEP_SUMMARY": str(summary),
            }
            with patch.dict(os.environ, env), patch.object(
                approval, "github_api", side_effect=self.api
            ):
                self.assertEqual(approval.main(), 0)
            values = dict(line.split("=", 1) for line in output.read_text().splitlines())
            self.assertEqual(values["approved"], "true")
            self.assertEqual(json.loads(values["evidence"])["spec_pull_requests"], [44])
            self.assertIn("#44", summary.read_text())

    def test_cli_fails_without_emitting_approval_on_api_error(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "output"
            summary = Path(directory) / "summary"
            env = {
                "GITHUB_REPOSITORY": REPOSITORY,
                "ISSUE_NUMBER": "42",
                "DEFAULT_BRANCH": "main",
                "GITHUB_OUTPUT": str(output),
                "GITHUB_STEP_SUMMARY": str(summary),
            }
            with patch.dict(os.environ, env), patch.object(
                approval, "github_api", side_effect=ValueError("GitHub API unavailable")
            ):
                self.assertEqual(approval.main(), 1)
            self.assertFalse(output.exists())
            self.assertIn("GitHub API unavailable", summary.read_text())

    @patch.object(subprocess, "run")
    def test_api_errors_are_reported_without_subprocess_stderr(self, run):
        run.side_effect = subprocess.CalledProcessError(
            1, ["gh"], stderr="private diagnostic"
        )
        with self.assertRaises(ValueError) as error:
            approval.github_api("repos/owner/acme/pulls/44")
        self.assertNotIn("private diagnostic", str(error.exception))


if __name__ == "__main__":
    unittest.main()
