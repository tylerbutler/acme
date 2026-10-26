import importlib.util
import json
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parent.parent / "scripts/bootstrap-github.py"
SPEC = importlib.util.spec_from_file_location("bootstrap_github", SCRIPT)
bootstrap = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bootstrap)


class RulesetTests(unittest.TestCase):
    def setUp(self):
        self.ruleset = {"name": "main", "enforcement": "active"}
        self.endpoint = "repos/owner/acme/rulesets"

    @patch.object(bootstrap, "gh")
    def test_creates_missing_ruleset(self, gh):
        gh.return_value = "[[]]"
        bootstrap.apply_ruleset("owner/acme", self.ruleset)
        gh.assert_any_call("api", "--method", "POST", self.endpoint,
                           "--input", "-", input_data=self.ruleset)

    @patch.object(bootstrap, "gh")
    def test_updates_named_ruleset_without_creating_another(self, gh):
        gh.return_value = json.dumps([
            [{"id": 1, "name": "other"}],
            [{"id": 42, "name": "main"}],
        ])
        bootstrap.apply_ruleset("owner/acme", self.ruleset)
        gh.assert_called_with("api", "--method", "PUT", f"{self.endpoint}/42",
                              "--input", "-", input_data=self.ruleset)
        self.assertEqual(gh.call_count, 2)

    @patch.object(bootstrap, "gh")
    def test_rejects_ambiguous_rulesets_before_writing(self, gh):
        gh.return_value = '[[{"id": 1, "name": "main"}, {"id": 2, "name": "main"}]]'
        with self.assertRaisesRegex(ValueError, "Multiple rulesets"):
            bootstrap.apply_ruleset("owner/acme", self.ruleset)
        self.assertEqual(gh.call_count, 1)


class WorkflowTests(unittest.TestCase):
    @patch.object(bootstrap, "gh")
    def test_disables_active_workflow(self, gh):
        gh.return_value = '{"state": "active"}'
        bootstrap.disable_workflow("owner/acme", "intake.lock.yml")
        gh.assert_called_with("workflow", "disable", "intake.lock.yml",
                              "--repo", "owner/acme")

    @patch.object(bootstrap, "gh")
    def test_does_not_disable_an_already_disabled_workflow(self, gh):
        gh.return_value = '{"state": "disabled_manually"}'
        bootstrap.disable_workflow("owner/acme", "intake.lock.yml")
        gh.assert_called_once_with(
            "api", "repos/owner/acme/actions/workflows/intake.lock.yml"
        )

    @patch.object(bootstrap, "gh")
    def test_rejects_deleted_workflow(self, gh):
        gh.return_value = '{"state": "deleted"}'
        with self.assertRaisesRegex(ValueError, "Unexpected state"):
            bootstrap.disable_workflow("owner/acme", "intake.lock.yml")
        self.assertEqual(gh.call_count, 1)


if __name__ == "__main__":
    unittest.main()
