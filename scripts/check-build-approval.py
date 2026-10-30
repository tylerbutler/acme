#!/usr/bin/env python3
"""Check Build-cloud spec evidence without assigning work."""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import quote, urlsplit


def github_api(endpoint):
    try:
        result = subprocess.run(
            ["gh", "api", "--method", "GET", endpoint],
            capture_output=True,
            text=True,
            check=True,
            timeout=30,
        )
        return json.loads(result.stdout)
    except (subprocess.SubprocessError, OSError, json.JSONDecodeError) as error:
        raise ValueError(f"Cannot read GitHub approval evidence: {endpoint}") from error


def spec_links(body, repository):
    specs = set()
    pulls = set()
    for line in body.splitlines():
        is_spec_pr = re.match(
            r"^\s*(?:[-*]\s+)?(?:\[\s*)?(?:(?:merged|approved|proposed)\s+)?"
            r"spec\s+pr\s*:",
            line,
            re.IGNORECASE,
        ) is not None
        urls = re.findall(r"https://github\.com/[^\s<>()\[\]]+", line)
        found_spec_pr = False
        for value in urls:
            url = urlsplit(value.rstrip(".,;"))
            path = url.path.removeprefix("/")
            prefix = f"{repository}/blob/"
            if path.startswith(prefix) and "/docs/specs/" in path[len(prefix):]:
                spec = "docs/specs/" + path.split("/docs/specs/", 1)[1]
                parts = spec.split("/")
                if (
                    not re.fullmatch(r"[A-Za-z0-9_./-]+\.md", spec)
                    or any(part in ("", ".", "..") for part in parts)
                ):
                    raise ValueError("Invalid linked spec path")
                specs.add(spec)
            if is_spec_pr:
                match = re.fullmatch(r"([^/]+/[^/]+)/pull/([1-9][0-9]*)", path)
                if match:
                    if match[1] != repository:
                        raise ValueError("Spec PR belongs to another repository")
                    pulls.add(int(match[2]))
                    found_spec_pr = True
        if is_spec_pr and not found_spec_pr:
            raise ValueError("Missing or invalid explicit spec PR link")
    if not specs:
        raise ValueError(
            "Missing linked spec under docs/specs/ in this repository. "
            "Add a full GitHub file URL, for example "
            f"https://github.com/{repository}/blob/HEAD/docs/specs/NAME.md. "
            "A bare path or PR link is not sufficient."
        )
    return sorted(specs), sorted(pulls)


def check_approval(repository, issue_number, default_branch, api):
    if (
        not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository)
        or issue_number <= 0
        or not default_branch
    ):
        raise ValueError("Invalid repository, issue or default branch")
    issue = api(f"repos/{repository}/issues/{issue_number}")
    if not isinstance(issue, dict) or not isinstance(issue.get("body"), str):
        raise ValueError("Missing issue body")
    specs, pulls = spec_links(issue["body"], repository)
    for spec in specs:
        evidence = api(
            f"repos/{repository}/contents/{spec}?ref={quote(default_branch, safe='')}"
        )
        if (
            not isinstance(evidence, dict)
            or evidence.get("type") != "file"
            or evidence.get("path") != spec
        ):
            raise ValueError(f"Missing spec on {default_branch}: {spec}")
    for number in pulls:
        pull = api(f"repos/{repository}/pulls/{number}")
        if not isinstance(pull, dict) or pull.get("merged") is not True:
            raise ValueError(f"Spec PR #{number} has no verified merge")
        base = pull.get("base")
        if (
            not isinstance(base, dict)
            or base.get("ref") != default_branch
            or not isinstance(base.get("repo"), dict)
            or base["repo"].get("full_name") != repository
        ):
            raise ValueError(f"Spec PR #{number} has an invalid base")
        matches_spec = False
        for page in range(1, 31):
            files = api(
                f"repos/{repository}/pulls/{number}/files?per_page=100&page={page}"
            )
            if not isinstance(files, list) or any(
                not isinstance(item, dict) for item in files
            ):
                raise ValueError(f"Cannot verify linked spec in PR #{number}")
            matches_spec = any(
                item.get("filename") in specs
                and item.get("status") in ("added", "modified", "renamed", "changed")
                for item in files
            )
            if matches_spec or len(files) < 100:
                break
        if not matches_spec:
            raise ValueError(f"Spec PR #{number} does not change a linked spec")
    return {
        "repository": repository,
        "issue": issue_number,
        "branch": default_branch,
        "specs": specs,
        "spec_pull_requests": pulls,
    }


def main(argv=()):
    preflight = bool(argv)
    if preflight:
        parser = argparse.ArgumentParser(
            description=(
                "Check an issue's spec evidence through read-only GitHub requests. "
                "This does not approve scope, assign work or change labels. "
                "With no arguments, use the existing GitHub Actions environment."
            )
        )
        parser.add_argument("--repository", required=True, help="OWNER/REPO")
        parser.add_argument("--issue", required=True, type=int, help="Issue number")
        parser.add_argument(
            "--default-branch", required=True, help="Repository default branch"
        )
        args = parser.parse_args(argv)
    try:
        evidence = check_approval(
            args.repository if preflight else os.environ["GITHUB_REPOSITORY"],
            args.issue if preflight else int(os.environ["ISSUE_NUMBER"]),
            args.default_branch if preflight else os.environ["DEFAULT_BRANCH"],
            github_api,
        )
    except (KeyError, ValueError) as error:
        summary = f"## Build approval blocked\n\n{error}\n"
        result = 1
    else:
        if not preflight:
            with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
                output.write(f"approved=true\nevidence={json.dumps(evidence)}\n")
        pulls = ", ".join(f"#{number}" for number in evidence["spec_pull_requests"])
        heading = "Build spec preflight passed" if preflight else "Build approval verified"
        summary = (
            f"## {heading}\n\n"
            f"Specs on `{evidence['branch']}`: {', '.join(evidence['specs'])}\n\n"
            f"Verified merged spec PRs: {pulls or 'none linked (main-only spec)'}\n"
        )
        result = 0
    if not preflight:
        with Path(os.environ["GITHUB_STEP_SUMMARY"]).open("a") as output:
            output.write(summary)
    print(summary, file=sys.stderr if result else sys.stdout)
    return result


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
