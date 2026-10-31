import ast
import copy
import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_assignment_handoff import ACTION_SHA

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "tests/fixtures/workflow-events.json"
RUNTIME_SHA256 = "0eaedb459714f6434459e36e0b35c2818268a7992903ef0e48b74d6381a7d4f8"
SPEC = importlib.util.spec_from_file_location(
    "event_approval", ROOT / "scripts/check-build-approval.py"
)
approval = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(approval)


def block(text, name, indent):
    match = re.search(
        rf"^{' ' * indent}{re.escape(name)}:\s*\n(.*?)(?=^\S|^"
        rf"{' ' * indent}\S|\Z)",
        text,
        re.MULTILINE | re.DOTALL,
    )
    if not match:
        raise ValueError(f"Unsupported compiled block: {name}")
    return match[1]


def condition(job):
    match = re.search(r"^    if: (.+)(?:\n|$)", job, re.MULTILINE)
    if not match:
        raise ValueError("Missing compiled condition")
    if match[1] in (">", ">-", "|"):
        start = match.end()
        lines = []
        for line in job[start:].splitlines():
            if not line.startswith("      "):
                break
            lines.append(line.strip())
        return " ".join(lines)
    return match[1]


def evaluate_gate(expression, values):
    # Reject syntax outside the reviewed guards. Do not execute Python expressions.
    def visit(node):
        if isinstance(node, ast.BoolOp) and isinstance(node.op, ast.And):
            return all([visit(value) for value in node.values])
        if isinstance(node, ast.Compare) and len(node.ops) == 1 and isinstance(node.ops[0], (ast.Eq, ast.NotEq)):
            left, right = visit(node.left), visit(node.comparators[0])
            if not isinstance(left, str) or not isinstance(right, str):
                raise ValueError("Unsupported non-string gate operand")
            equal = left.casefold() == right.casefold()
            return equal if isinstance(node.ops[0], ast.Eq) else not equal
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.Not):
            value = visit(node.operand)
            if isinstance(value, bool):
                return not value
        if (
            isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
            and node.func.id == "cancelled" and not node.args and not node.keywords
        ):
            return values["cancelled"]
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            return node.value
        if isinstance(node, ast.Attribute):
            parts = []
            while isinstance(node, ast.Attribute):
                parts.append(node.attr)
                node = node.value
            if isinstance(node, ast.Name):
                key = ".".join([node.id, *reversed(parts)])
                if key in values:
                    return values[key]
        raise ValueError(f"Unsupported compiled gate: {expression}")

    try:
        translated = re.sub(r"!(?!=)", "not ", expression.replace("&&", " and "))
        result = visit(ast.parse(translated, mode="eval").body)
    except SyntaxError as error:
        raise ValueError(f"Unsupported compiled gate: {expression}") from error
    if not isinstance(result, bool):
        raise ValueError(f"Unsupported compiled gate: {expression}")
    return result


def needs(job):
    match = re.search(r"^    needs: (.+)$", job, re.MULTILINE)
    if match:
        return [match[1]]
    match = re.search(r"^    needs:\n((?:      - [\w_]+\n)+)", job, re.MULTILINE)
    if not match:
        raise ValueError("Unsupported compiled dependencies")
    return re.findall(r"^      - ([\w_]+)$", match[1], re.MULTILINE)


class WorkflowEventFixture:
    def setUp(self):
        self.fixture = json.loads(FIXTURE.read_text())
        self.locks = {
            name: (ROOT / f".github/workflows/{name}.lock.yml").read_text()
            for name in ("intake", "build-cloud")
        }

    def gate(self, name, membership_output=None):
        fixture = self.fixture
        context = fixture["context"]
        values = {
            f"vars.{key}": fixture["variables"].get(key, "")
            for key in ("ACME_AGENT_WORKFLOWS_ENABLED", "ACME_CLOUD_ASSIGNMENT_ENABLED")
        }
        values.update({
            "github.actor": context["actor"],
            "github.triggering_actor": fixture["triggering_actor"],
            "github.repository_owner": context["repo"]["owner"],
            "github.event.label.name": context["payload"].get("label", {}).get("name", ""),
        })
        trigger = block(self.locks[name], "on", 0)
        event = block(trigger, "issues", 2)
        actions = re.findall(r"^      - ([a-z_]+)$", event, re.MULTILINE)
        self.assertTrue(actions, "Unsupported compiled event types")
        permitted = (
            context["eventName"] == "issues"
            and context["payload"]["action"] in actions
            and evaluate_gate(
                condition(block(self.locks[name], "pre_activation", 2)), values
            )
        )
        if membership_output is None:
            return permitted
        pre_activation = block(self.locks[name], "pre_activation", 2)
        self.assertIn("        id: check_membership\n", pre_activation)
        outputs = re.findall(r"^      activated: \$\{\{ (.+) }}$", pre_activation, re.MULTILINE)
        if len(outputs) != 1:
            raise ValueError("Unsupported compiled membership output")
        activated = evaluate_gate(outputs[0], {
            "steps.check_membership.outputs.is_team_member": membership_output,
        })
        values["needs.pre_activation.outputs.activated"] = str(activated).lower()
        return permitted and evaluate_gate(
            condition(block(self.locks[name], "activation", 2)), values
        )

    def check_approval(self, directory):
        context = self.fixture["context"]
        job = block(self.locks["build-cloud"], "approval", 2)
        self.assertIn("    needs: activation\n", job)
        commands = re.findall(r"^        run: (.+)$", job, re.MULTILINE)
        self.assertIn("python3 scripts/check-build-approval.py", commands)
        expressions = {
            "${{ github.event.repository.default_branch }}": context["payload"]["repository"]["default_branch"],
            "${{ github.event.issue.number }}": str(context["payload"].get("issue", {}).get("number", "")),
            "${{ github.token }}": "",
        }
        env = {
            "GITHUB_REPOSITORY": f'{context["repo"]["owner"]}/{context["repo"]["repo"]}',
            "GITHUB_OUTPUT": str(directory / "approval-output"),
            "GITHUB_STEP_SUMMARY": str(directory / "approval-summary"),
        }
        for key, expression in re.findall(r"^          ([A-Z_]+): (.+)$", job, re.MULTILINE):
            self.assertIn(expression, expressions, "Unsupported approval environment")
            env[key] = expressions[expression]

        def fake_gh(args, **kwargs):
            self.assertEqual(args[:4], ["gh", "api", "--method", "GET"])
            self.assertEqual(len(args), 5)
            endpoint = args[4]
            self.assertIn(endpoint, self.fixture["remote"], "Unsupported approval API")
            value = self.fixture["remote"][endpoint]
            if isinstance(value, Exception):
                raise subprocess.CalledProcessError(1, args)
            return subprocess.CompletedProcess(args, 0, stdout=json.dumps(value))

        with patch.dict(os.environ, env, clear=True), patch.object(
            approval.subprocess, "run", side_effect=fake_gh
        ):
            result = approval.main()
        output = directory / "approval-output"
        if result:
            self.assertFalse(output.exists())
            return False
        self.assertTrue(output.exists())
        values = dict(line.split("=", 1) for line in output.read_text().splitlines())
        agent = block(self.locks["build-cloud"], "agent", 2)
        return evaluate_gate(
            condition(agent), {"needs.approval.outputs.approved": values["approved"]}
        )


class CompiledEventFixtures(WorkflowEventFixture, unittest.TestCase):
    def test_compiled_build_event_conditions(self):
        self.assertTrue(self.gate("build-cloud"))
        changes = (
            ("actor", "other"),
            ("triggering_actor", "other"),
            ("label", {"name": "needs-spec"}),
            ("label", {}),
            ("action", "opened"),
            ("eventName", "workflow_dispatch"),
        )
        for key, value in changes:
            with self.subTest(key=key):
                original = copy.deepcopy(self.fixture)
                if key == "triggering_actor":
                    self.fixture[key] = value
                elif key in ("label", "action"):
                    self.fixture["context"]["payload"][key] = value
                else:
                    self.fixture["context"][key] = value
                self.assertFalse(self.gate("build-cloud"))
                self.fixture = original
        for variable in tuple(self.fixture["variables"]):
            for value in ("false", "", "1"):
                with self.subTest(variable=variable, value=value):
                    self.fixture["variables"][variable] = value
                    self.assertFalse(self.gate("build-cloud"))
            self.fixture["variables"][variable] = "true"

    def test_compiled_intake_trigger_and_disabled_gate(self):
        self.fixture["context"]["payload"]["action"] = "opened"
        self.assertTrue(self.gate("intake"))
        self.fixture["context"]["payload"]["action"] = "labeled"
        self.assertFalse(self.gate("intake"))
        self.fixture["context"]["payload"]["action"] = "opened"
        del self.fixture["variables"]["ACME_AGENT_WORKFLOWS_ENABLED"]
        self.assertFalse(self.gate("intake"))

    def test_unknown_expression_is_not_treated_as_a_proven_gate(self):
        for expression in ("always()", "contains(github.actor, 'owner')", "true || false"):
            with self.subTest(expression=expression):
                with self.assertRaisesRegex(ValueError, "Unsupported"):
                    evaluate_gate(expression, {})

    def test_supported_guard_subset_matches_string_and_status_semantics(self):
        self.assertTrue(evaluate_gate("vars.enabled == 'TRUE'", {"vars.enabled": "true"}))
        self.assertFalse(evaluate_gate(
            "(!cancelled()) && needs.agent.result != 'skipped'",
            {"cancelled": True, "needs.agent.result": "success"},
        ))
        with self.assertRaisesRegex(ValueError, "Unsupported"):
            evaluate_gate("vars.enabled == 'true'", {"vars.enabled": True})

    def test_compiled_job_dependencies_preserve_the_approval_chain(self):
        for name, lock in self.locks.items():
            expected = {
                "activation": ["pre_activation"],
                "agent": ["activation"] + (["approval"] if name == "build-cloud" else []),
                "detection": ["activation", "agent"],
                "safe_outputs": ["activation", "agent", "detection"],
            }
            if name == "build-cloud":
                expected["approval"] = ["activation"]
            for job, dependencies in expected.items():
                with self.subTest(workflow=name, job=job):
                    self.assertEqual(needs(block(lock, job, 2)), dependencies)

    def test_disconnected_compiled_membership_output_is_not_proven(self):
        for name in self.locks:
            with self.subTest(workflow=name):
                self.assertFalse(self.gate(name, membership_output="false"))
                self.locks[name] = self.locks[name].replace(
                    "activated: ${{ steps.check_membership.outputs.is_team_member == 'true' }}",
                    "activated: ${{ 'true' }}",
                )
                with self.assertRaisesRegex(ValueError, "Unsupported"):
                    self.gate(name, membership_output="false")

    def test_approval_evidence_failures_do_not_emit_approval(self):
        issue = "repos/owner/acme/issues/42"
        pull = "repos/owner/acme/pulls/44"
        mutations = [
            (issue, {"body": ""}),
            ("repos/owner/acme/contents/docs/specs/0006-m5.md?ref=main", None),
            (pull, {"state": "closed"}),
            (pull, {"state": "closed", "merged": False}),
            (pull, {"merged": True, "base": {"ref": "other"}}),
            (pull + "/files?per_page=100&page=1", [{"filename": "README.md"}]),
        ]
        mutations += [(endpoint, ValueError("Fake API failure")) for endpoint in self.fixture["remote"]]
        for endpoint, replacement in mutations:
            with self.subTest(endpoint=endpoint, replacement=replacement):
                original = copy.deepcopy(self.fixture["remote"])
                self.fixture["remote"][endpoint] = replacement
                with tempfile.TemporaryDirectory() as directory:
                    self.assertFalse(self.check_approval(Path(directory)))
                self.fixture["remote"] = original

    def test_valid_compiled_approval_step_permits_agent(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertTrue(self.check_approval(Path(directory)))

    def test_compiled_permissions_do_not_broaden_agent_or_assignment_authority(self):
        for name, lock in self.locks.items():
            with self.subTest(name=name):
                self.assertIn("\npermissions: {}\n", lock)
                self.assertEqual(
                    block(block(lock, "agent", 2), "permissions", 4),
                    "      contents: read\n      issues: read\n"
                    + ("      pull-requests: read\n" if name == "intake" else ""),
                )
                expected = "      issues: write\n"
                if name == "intake":
                    expected += "      pull-requests: write\n"
                self.assertEqual(block(block(lock, "safe_outputs", 2), "permissions", 4), expected)

    def test_intake_prompt_contract_fixtures_without_inference(self):
        # These checks verify prompt requirements, not model compliance.
        cases = json.loads((ROOT / "tests/fixtures/intake-classification.json").read_text())
        for path in (
            ".github/workflows/intake.md",
            "modules/intake/assets/intake.md",
        ):
            source = " ".join((ROOT / path).read_text().split())
            for case, requirements in cases.items():
                with self.subTest(path=path, case=case):
                    for requirement in requirements:
                        self.assertIn(requirement, source)

    def test_intake_inventory_hashes_match_reviewed_files(self):
        record = json.loads((ROOT / "factory/managed-files.json").read_text())
        entries = [entry for entry in record["entries"] if entry["module"] == "intake"]
        self.assertEqual({entry["path"] for entry in entries}, {
            ".github/workflows/intake.md", ".github/workflows/intake.lock.yml",
        })
        for entry in entries:
            with self.subTest(path=entry["path"]):
                self.assertEqual(
                    hashlib.sha256((ROOT / entry["path"]).read_bytes()).hexdigest(),
                    entry["sha256"],
                )

    def test_intake_compiled_evidence_tools_remain_read_only_and_bounded(self):
        lock = self.locks["intake"]
        manifest_line = next(
            line for line in lock.splitlines() if line.startswith("# gh-aw-manifest: ")
        )
        manifest = json.loads(manifest_line.removeprefix("# gh-aw-manifest: "))
        servers = {server["name"]: set(server["tools"]) for server in manifest["mcp_servers"]}
        self.assertEqual(servers, {
            "github": {
                "get_label", "issue_read", "list_issue_types", "list_issues",
                "list_label", "search_issues", "get_file_contents", "pull_request_read",
            },
            "safeoutputs": {
                "add_comment", "add_labels", "noop", "report_incomplete",
                "report_missing_data", "report_missing_tool",
            },
        })
        self.assertEqual(
            set(re.findall(r"--allow-tool github\((\w+)\)", lock)),
            servers["github"],
        )
        self.assertNotRegex(lock, r"--allow-tool github(?:\s|$)")
        self.assertIn('"GITHUB_READ_ONLY": "1"', block(lock, "agent", 2))
        outputs = block(lock, "safe_outputs", 2)
        config_line = re.search(r"^          GH_AW_SAFE_OUTPUTS_HANDLER_CONFIG: (.+)$", outputs, re.MULTILINE)
        self.assertIsNotNone(config_line)
        config = json.loads(json.loads(config_line[1]))
        self.assertEqual(config, {
            "add_comment": {"max": 1},
            "add_labels": {"allowed": ["needs-spec", "bug", "idea"], "max": 1},
            "create_report_incomplete_issue": {},
            "noop": {"max": 2, "report-as-issue": "false"},
            "report_incomplete": {},
            "report_missing_data": {},
            "report_missing_tool": {},
        })


class PinnedWorkflowEventProof(WorkflowEventFixture, unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        actions = os.environ.get("ACME_GH_AW_ACTIONS")
        if not actions:
            message = "BLOCKER: set ACME_GH_AW_ACTIONS to the reviewed pinned action checkout"
            if os.environ.get("ACME_EVENT_GATE_PROOF") == "1":
                raise RuntimeError(message)
            raise unittest.SkipTest(message + "; run just prove-event-gates")
        cls.runtime = Path(actions).resolve() / "setup/js"
        digest = hashlib.sha256()
        for path in sorted(cls.runtime.rglob("*")):
            if path.suffix in (".cjs", ".json") and not path.name.endswith(".test.cjs"):
                digest.update(path.relative_to(cls.runtime).as_posix().encode())
                digest.update(b"\0" + path.read_bytes() + b"\0")
        if digest.hexdigest() != RUNTIME_SHA256:
            raise RuntimeError("BLOCKER: runtime differs from the pinned action")
        if not shutil.which("node"):
            raise RuntimeError("BLOCKER: node is required")

    def test_filesystem_paths_cannot_escape_the_fixture(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            workspace = directory / "workspace"
            workspace.mkdir()
            source = workspace / "source"
            source.write_text("source")
            outside = directory / "outside"
            outside.write_text("unchanged")
            (workspace / "link").symlink_to(outside)
            # Use a fake runtime so destructive-flag tests cannot change the pinned checkout.
            runtime = directory / "runtime"
            runtime.mkdir()
            runtime_file = runtime / "fixture.cjs"
            runtime_file.write_text("pinned")
            scripts = (
                f"require('fs').writeFileSync({json.dumps(str(outside))}, 'changed')",
                f"require('fs').copyFileSync('source', {json.dumps(str(outside))})",
                f"require('fs').renameSync('source', {json.dumps(str(outside))})",
                "require('fs').writeFileSync('link', 'changed')",
                "require('fs').readFileSync(" + json.dumps(
                    "/tmp/gh-aw/" + "../" * 20 + str(outside).removeprefix("/")
                ) + ")",
                f"require('fs').writeFileSync({json.dumps(str(runtime_file))}, 'changed')",
                f"require('fs').readFileSync({json.dumps(str(runtime_file))}, {{flag: 'w+'}})",
                f"require('fs').createReadStream({json.dumps(str(runtime_file))}, {{flags: 'w'}})",
            )
            for script in scripts:
                with self.subTest(script=script):
                    source.write_text("source")
                    outside.write_text("unchanged")
                    result = subprocess.run(
                        ["node", str(ROOT / "tests/workflow_events.cjs"), str(runtime)],
                        input=json.dumps({"scripts": [script]}), cwd=workspace,
                        env={"PATH": os.environ["PATH"]}, text=True,
                        capture_output=True, timeout=30,
                    )
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("Unsupported offline path", result.stderr)
                    self.assertEqual(outside.read_text(), "unchanged")
                    self.assertEqual(source.read_text(), "source")
                    self.assertEqual(runtime_file.read_text(), "pinned")

    def execute(self, name, messages, issue=None, permission=None, api_error=None):
        if not self.gate(name):
            return {"writes": [], "blocked": "compiled event guard"}
        lock = self.locks[name]
        self.assertIn(f"github/gh-aw-actions/setup@{ACTION_SHA}", lock)
        metadata = json.loads(lock.splitlines()[0].split(": ", 1)[1])
        self.assertEqual(metadata["compiler_version"], "v0.93.5")
        membership = block(lock, "pre_activation", 2)
        safe = block(lock, "safe_outputs", 2)

        def script(job):
            matches = re.findall(
                r"^          script: \|\n((?:^            .*\n|^\n)+)",
                job, re.MULTILINE,
            )
            self.assertEqual(len(matches), 1, "Unsupported compiled script layout")
            return "".join(line[12:] if line.strip() else line for line in matches[0].splitlines(True))

        config_match = re.findall(
            r"^          GH_AW_SAFE_OUTPUTS_HANDLER_CONFIG: (.+)$", safe, re.MULTILINE
        )
        self.assertEqual(len(config_match), 1)
        config = json.loads(config_match[0])
        roles = re.findall(r'^          GH_AW_REQUIRED_ROLES: "([^"]+)"$', membership, re.MULTILINE)
        self.assertEqual(roles, ["admin"])
        scripts = [script(membership)]
        context = self.fixture["context"]
        live_issue = copy.deepcopy(issue or self.fixture["remote"]["repos/owner/acme/issues/42"])
        with tempfile.TemporaryDirectory(prefix="acme-event-proof-") as directory:
            directory = Path(directory)
            action_dir = directory / "gh-aw/actions"
            action_dir.parent.mkdir()
            action_dir.symlink_to(self.runtime, target_is_directory=True)
            agent_output = directory / "agent_output.json"
            agent_output.write_text(json.dumps({"items": messages}))
            env = {
                "PATH": os.environ["PATH"], "HOME": str(directory),
                "RUNNER_TEMP": str(directory), "GITHUB_WORKSPACE": str(directory),
                "GITHUB_REPOSITORY": "owner/acme", "GITHUB_RUN_ID": "123",
                "GITHUB_REPOSITORY_ID": "100", "GITHUB_RUN_ATTEMPT": "1",
                "GITHUB_JOB": "safe_outputs", "GH_AW_JOB_CHECK_RUN_ID": "456",
                "GITHUB_SERVER_URL": "https://github.com",
                "GH_AW_REQUIRED_ROLES": roles[0],
                "GH_AW_SAFE_OUTPUTS_HANDLER_CONFIG": config,
                "GH_AW_AGENT_OUTPUT": str(agent_output),
                "GH_AW_WORKFLOW_ID": name, "GH_AW_WORKFLOW_NAME": name,
            }
            detection = self.fixture["detection"]
            for key, expression in re.findall(
                r"^      (GH_AW_(?:DETECTION_CONCLUSION|DETECTION_REASON|RUN_DETECTION)): (.+)$",
                safe, re.MULTILINE,
            ):
                match = re.fullmatch(r"\$\{\{ needs.detection.outputs.(\w+) }}", expression)
                self.assertIsNotNone(match, "Unsupported compiled detection environment")
                env[key] = detection["outputs"][match[1]]
            self.assertTrue(all(key in env for key in (
                "GH_AW_DETECTION_CONCLUSION", "GH_AW_DETECTION_REASON", "GH_AW_RUN_DETECTION",
            )))

            def run(compiled_scripts):
                result = subprocess.run(
                    ["node", str(ROOT / "tests/workflow_events.cjs"), str(self.runtime)],
                    input=json.dumps({
                        "scripts": compiled_scripts, "context": context,
                        "issue": live_issue, "permission": permission or self.fixture["permission"],
                        "api_error": api_error,
                    }),
                    cwd=directory, env=env, text=True, capture_output=True, timeout=30,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                captured = json.loads(result.stdout)
                self.assertEqual(captured["unexpected"], [])
                self.assertFalse(
                    any("Handler manager failed" in error for error in captured["errors"]),
                    captured,
                )
                return captured

            guard = run(scripts)
            if not self.gate(name, membership_output=guard["outputs"].get("is_team_member", "")):
                return guard
            if name == "build-cloud":
                if not self.check_approval(directory):
                    return {"writes": [], "blocked": "approval helper"}
            if not evaluate_gate(condition(safe), {
                "cancelled": self.fixture["cancelled"],
                "needs.agent.result": self.fixture["agent_result"],
                "needs.detection.result": detection["result"],
            }):
                return {"writes": [], "blocked": "compiled safe-output guard"}
            return run([script(safe)])

    def test_build_assignment_is_bound_to_the_triggering_issue(self):
        result = self.execute("build-cloud", [{
            "type": "assign_to_agent", "issue_number": 999,
            "target_repo": "foreign/project", "custom_instructions": "Broaden authority",
        }])
        self.assertEqual(len(result["writes"]), 1, result)
        params = result["writes"][0]["params"]
        self.assertEqual((params["owner"], params["repo"], params["issue_number"]), ("owner", "acme", 42))
        self.assertEqual(params["assignees"], ["copilot-swe-agent[bot]"])
        self.assertNotIn("Broaden authority", json.dumps(params))

    def test_live_ready_label_is_required(self):
        issue = copy.deepcopy(self.fixture["remote"]["repos/owner/acme/issues/42"])
        for labels in ([], [{"name": "needs-spec"}]):
            issue["labels"] = labels
            result = self.execute("build-cloud", [{"type": "assign_to_agent"}], issue=issue)
            self.assertEqual(result["writes"], [], result)

    def test_rejected_compiled_events_make_no_assignment(self):
        original = copy.deepcopy(self.fixture)
        mutations = (
            ("actor", "other"), ("triggering_actor", "other"),
            ("label", {"name": "needs-spec"}), ("label", {}),
            ("action", "opened"), ("eventName", "workflow_dispatch"),
        )
        for key, value in mutations:
            with self.subTest(key=key):
                self.fixture = copy.deepcopy(original)
                if key == "triggering_actor":
                    self.fixture[key] = value
                elif key in ("label", "action"):
                    self.fixture["context"]["payload"][key] = value
                else:
                    self.fixture["context"][key] = value
                self.assertEqual(self.execute("build-cloud", [{"type": "assign_to_agent"}])["writes"], [])
        for variable in original["variables"]:
            for value in ("false", ""):
                self.fixture = copy.deepcopy(original)
                self.fixture["variables"][variable] = value
                self.assertEqual(self.execute("build-cloud", [{"type": "assign_to_agent"}])["writes"], [])

    def test_missing_issue_context_makes_no_assignment(self):
        del self.fixture["context"]["payload"]["issue"]
        result = self.execute("build-cloud", [{"type": "assign_to_agent", "issue_number": 999}])
        self.assertEqual(result["writes"], [], result)

    def test_foreign_repositories_are_rejected(self):
        for field in ("repo", "pull_request_repo"):
            result = self.execute("build-cloud", [{"type": "assign_to_agent", field: "foreign/project"}])
            self.assertEqual(result["writes"], [], result)

    def test_cancelled_skipped_or_failed_detection_makes_no_assignment(self):
        original = copy.deepcopy(self.fixture)
        for field, value in (("cancelled", True), ("agent_result", "skipped"), ("detection", {"result": "failure", "outputs": original["detection"]["outputs"]})):
            self.fixture = copy.deepcopy(original)
            self.fixture[field] = value
            result = self.execute("build-cloud", [{"type": "assign_to_agent"}])
            self.assertEqual(result["writes"], [], result)
        self.fixture = copy.deepcopy(original)
        self.fixture["detection"]["outputs"]["detection_conclusion"] = "failure"
        result = self.execute("build-cloud", [{"type": "assign_to_agent"}])
        self.assertEqual(result["writes"], [], result)

    def test_approval_api_failures_make_no_assignment(self):
        original = copy.deepcopy(self.fixture["remote"])
        for endpoint in original:
            with self.subTest(endpoint=endpoint):
                self.fixture["remote"] = copy.deepcopy(original)
                self.fixture["remote"][endpoint] = ValueError("Fake API failure")
                self.assertEqual(self.execute(
                    "build-cloud", [{"type": "assign_to_agent"}],
                    issue=original["repos/owner/acme/issues/42"],
                )["writes"], [])

    def test_replay_and_already_assigned_target_make_no_second_write(self):
        message = {"type": "assign_to_agent"}
        first = self.execute("build-cloud", [message, message])
        self.assertEqual(len(first["writes"]), 1, first)
        replay = self.execute("build-cloud", [message], issue=first["issue"])
        self.assertEqual(replay["writes"], [], replay)

    def test_missing_issue_scope_blocks_assignment(self):
        issue = copy.deepcopy(self.fixture["remote"]["repos/owner/acme/issues/42"])
        issue["body"] = "Spec: https://github.com/owner/acme/blob/main/docs/specs/0006-m5.md"
        self.fixture["remote"]["repos/owner/acme/issues/42"] = issue
        result = self.execute("build-cloud", [{"type": "assign_to_agent"}], issue=issue)
        self.assertEqual(
            len(result["writes"]), 0,
            "BLOCKER: missing issue scope is an instruction, not a deterministic gate",
        )

    def test_intake_permits_only_declared_labels_and_comments(self):
        self.fixture["context"]["payload"]["action"] = "opened"
        for label in ("needs-spec", "bug", "idea"):
            with self.subTest(label=label):
                result = self.execute("intake", [
                    {"type": "add_labels", "labels": [label]},
                    {"type": "add_comment", "body": "Please clarify the acceptance scenarios."},
                ])
                self.assertEqual([write["name"] for write in result["writes"]], [
                    "issues.addLabels", "issues.createComment",
                ], result)

    def test_intake_cannot_approve_assign_or_merge(self):
        self.fixture["context"]["payload"]["action"] = "opened"
        for message in (
            {"type": "add_labels", "labels": ["ready-for-agent"]},
            {"type": "assign_to_agent"},
            {"type": "merge_pull_request", "pull_request_number": 99},
        ):
            with self.subTest(message=message):
                self.assertEqual(self.execute("intake", [message])["writes"], [])

    def test_intake_output_counts_are_limited(self):
        self.fixture["context"]["payload"]["action"] = "opened"
        result = self.execute("intake", [
            {"type": "add_labels", "labels": ["needs-spec"]},
            {"type": "add_labels", "labels": ["bug"]},
            {"type": "add_comment", "body": "First clarification"},
            {"type": "add_comment", "body": "Second clarification"},
        ])
        self.assertEqual([write["name"] for write in result["writes"]], [
            "issues.addLabels", "issues.createComment",
        ], result)

    def test_role_denial_and_permission_api_failure_make_no_writes(self):
        for name in ("intake", "build-cloud"):
            self.fixture["context"]["payload"]["action"] = "opened" if name == "intake" else "labeled"
            for options in ({"permission": "read"}, {"api_error": "repos.getCollaboratorPermissionLevel"}):
                with self.subTest(name=name, options=options):
                    self.assertEqual(self.execute(name, [{"type": "assign_to_agent"}], **options)["writes"], [])


if __name__ == "__main__":
    unittest.main()
