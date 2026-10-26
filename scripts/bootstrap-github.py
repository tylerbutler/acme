#!/usr/bin/env python3
"""Apply the hand-built M0 settings without enabling agent workflows."""

import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def gh(*args, input_data=None):
    return subprocess.run(
        ["gh", *args],
        cwd=ROOT,
        input=json.dumps(input_data) if input_data is not None else None,
        stdout=subprocess.PIPE,
        text=True,
        check=True,
    ).stdout


def apply_ruleset(repo, ruleset):
    endpoint = f"repos/{repo}/rulesets"
    existing = json.loads(gh("api", endpoint, "--paginate", "--slurp"))
    matches = [
        item for page in existing for item in page if item["name"] == ruleset["name"]
    ]
    if len(matches) > 1:
        raise ValueError(f"Multiple rulesets named {ruleset['name']!r}; resolve them first")
    if matches:
        gh("api", "--method", "PUT", f"{endpoint}/{matches[0]['id']}",
           "--input", "-", input_data=ruleset)
    else:
        gh("api", "--method", "POST", endpoint, "--input", "-", input_data=ruleset)


def main():
    repo = gh("repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner").strip()
    for variable in ("ACME_AGENT_WORKFLOWS_ENABLED", "ACME_CLOUD_ASSIGNMENT_ENABLED"):
        gh("variable", "set", variable, "--repo", repo, "--body", "false")
    for workflow in ("intake.lock.yml", "build-cloud.lock.yml"):
        gh("workflow", "disable", workflow, "--repo", repo)

    gh("api", "--method", "PATCH", f"repos/{repo}", "--input", "-", input_data={
        "allow_auto_merge": False,
        "allow_merge_commit": False,
        "allow_rebase_merge": False,
        "allow_squash_merge": True,
        "delete_branch_on_merge": True,
        "has_wiki": False,
    })
    gh("api", "--method", "PUT", f"repos/{repo}/actions/permissions/workflow",
       "--input", "-", input_data={
           "default_workflow_permissions": "read",
           "can_approve_pull_request_reviews": False,
       })

    labels = json.loads((ROOT / ".github/labels.yml").read_text())
    for label in labels:
        gh("label", "create", label["name"], "--repo", repo, "--force",
           "--color", label["color"], "--description", label["description"])

    ruleset = json.loads((ROOT / ".github/rulesets/main.json").read_text())
    apply_ruleset(repo, ruleset)
    print(f"Applied M0 settings to {repo}. Agent workflows remain disabled.")
    print("This script does not configure a PAT or run the live M0 acceptance check.")


if __name__ == "__main__":
    main()
