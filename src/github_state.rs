use crate::assignment::AssignmentRequest;
use crate::claims::{ActualWork, GitHubSnapshot, GitHubState, OwnerReady};
use crate::{
    AcceptanceReceipt, ApprovalEvidence, DispatchDeclaration, Evidence, ExistingWork,
    FactoryConfig, IssueState, KillSwitches, PrerequisiteCompletion, TaskObservation,
    content_sha256,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

mod client;
use client::encode;
pub use client::{GhClient, GitHubClient};

/// Use the manual reader only for an owner-label reconciliation.
pub struct GitHubStateReader<C> {
    client: C,
    manual: bool,
}

impl<C: GitHubClient> GitHubStateReader<C> {
    pub const fn new(client: C) -> Self {
        Self {
            client,
            manual: false,
        }
    }

    pub const fn manual(client: C) -> Self {
        Self {
            client,
            manual: true,
        }
    }

    fn get(&self, endpoint: &str) -> Result<Value, String> {
        self.client
            .get(endpoint)
            .map_err(|error| format!("{endpoint}: {error}"))
    }

    fn list(&self, endpoint: &str, field: Option<&str>) -> Result<Vec<Value>, String> {
        let mut rows = Vec::new();
        let mut seen = BTreeSet::new();
        let mut total = None;
        for page in 1..=1000 {
            let separator = if endpoint.contains('?') { '&' } else { '?' };
            let response = self.get(&format!("{endpoint}{separator}per_page=100&page={page}"))?;
            let count = if field == Some("tasks") {
                let key = if endpoint.contains("is_archived=true") {
                    "total_archived_count"
                } else {
                    "total_active_count"
                };
                Some(response.get(key).ok_or("missing agent task count")?)
            } else if field.is_some() {
                Some(response.get("total_count").ok_or("missing list count")?)
            } else {
                None
            };
            if let Some(count) = count {
                let count = count.as_u64().ok_or("invalid list total")?;
                if total.is_some_and(|previous| previous != count) {
                    return Err(format!("{endpoint}: list changed during pagination"));
                }
                total = Some(count);
            }
            let values = field.map_or(&response, |name| &response[name]);
            let values = values
                .as_array()
                .ok_or_else(|| format!("{endpoint}: incomplete list"))?;
            for value in values {
                if !seen.insert(value.to_string()) {
                    return Err(format!("{endpoint}: duplicate or repeated page"));
                }
                rows.push(value.clone());
            }
            if values.len() < 100 {
                if total.is_some_and(|count| count != rows.len() as u64) {
                    return Err(format!("{endpoint}: truncated list"));
                }
                return Ok(rows);
            }
        }
        Err(format!("{endpoint}: pagination limit reached"))
    }

    pub(crate) fn branch(&self, prefix: &str) -> Result<String, String> {
        let branch = self.get(&format!("{prefix}/branches/main"))?;
        if text(&branch, "/name")? != "main" || branch["protected"] != true {
            return Err("main is not protected".to_owned());
        }
        let revision = text(&branch, "/commit/sha")?;
        if !valid_revision(revision) {
            return Err("invalid main revision".to_owned());
        }
        Ok(revision.to_owned())
    }

    pub(crate) fn ancestor(
        &self,
        prefix: &str,
        ancestor: &str,
        revision: &str,
    ) -> Result<(), String> {
        if !valid_revision(ancestor) {
            return Err("invalid merge revision".to_owned());
        }
        let comparison = self.get(&format!("{prefix}/compare/{ancestor}...{revision}"))?;
        if !matches!(text(&comparison, "/status")?, "ahead" | "identical") {
            return Err("merge is not on protected main".to_owned());
        }
        Ok(())
    }

    fn human_merge(
        &self,
        repository: &str,
        number: u64,
        owner: &str,
        revision: &str,
    ) -> Result<Value, String> {
        let prefix = format!("repos/{repository}");
        let pr = self.get(&format!("{prefix}/pulls/{number}"))?;
        if number == 0
            || pr["number"] != number
            || pr["merged"] != true
            || pr["state"] != "closed"
            || pr["merged_at"].as_str().is_none()
            || text(&pr, "/base/repo/full_name")? != repository
            || text(&pr, "/base/ref")? != "main"
            || text(&pr, "/merged_by/type")? != "User"
            || !text(&pr, "/merged_by/login")?.eq_ignore_ascii_case(owner)
        {
            return Err("PR has no authorized human merge to main".to_owned());
        }
        self.ancestor(&prefix, text(&pr, "/merge_commit_sha")?, revision)?;
        Ok(pr)
    }

    pub(crate) fn file_approval(
        &self,
        repository: &str,
        path: &str,
        content: &str,
        owner: &str,
        revision: &str,
    ) -> Result<Value, String> {
        let prefix = format!("repos/{repository}");
        let commits = self.get(&format!(
            "{prefix}/commits?sha={revision}&path={}&per_page=1",
            encode(path)
        ))?;
        let commit = commits
            .as_array()
            .and_then(|rows| rows.first())
            .ok_or("missing file history")?;
        let sha = text(commit, "/sha")?;
        if !valid_revision(sha) {
            return Err("invalid file history revision".to_owned());
        }
        let associated = self.list(&format!("{prefix}/commits/{sha}/pulls"), None)?;
        let mut approvals = Vec::new();
        for candidate in associated {
            let number = positive(&candidate, "/number")?;
            if let Ok(pr) = self.human_merge(repository, number, owner, revision) {
                let files = self.list(&format!("{prefix}/pulls/{number}/files"), None)?;
                if pr["changed_files"].as_u64() != Some(files.len() as u64) {
                    return Err("PR file evidence is incomplete".to_owned());
                }
                if files.iter().any(|file| file["filename"] == path) {
                    let approved =
                        self.client
                            .file(repository, path, text(&pr, "/merge_commit_sha")?)?;
                    if content_sha256(&approved) == content_sha256(content) {
                        approvals.push(pr);
                    }
                }
            }
        }
        if approvals.len() != 1 {
            return Err("missing or ambiguous authorized file merge".to_owned());
        }
        Ok(approvals.remove(0))
    }

    fn closing_issues(&self, repository: &str, number: u64) -> Result<Vec<u64>, String> {
        let (owner, name) = repository.split_once('/').ok_or("invalid repository")?;
        let mut cursor = Value::Null;
        let mut seen = BTreeSet::new();
        let mut issues = BTreeSet::new();
        loop {
            let query = format!(
                "query {{ repository(owner: {owner:?}, name: {name:?}) {{ pullRequest(number: {number}) {{ closingIssuesReferences(first: 100, after: {cursor}) {{ nodes {{ number repository {{ nameWithOwner }} }} pageInfo {{ hasNextPage endCursor }} }} }} }} }}"
            );
            let result = self.client.graphql(&query)?;
            if result.get("errors").is_some() {
                return Err("incomplete PR closing references".to_owned());
            }
            let connection = result
                .pointer("/data/repository/pullRequest/closingIssuesReferences")
                .ok_or("missing PR closing references")?;
            for row in connection["nodes"]
                .as_array()
                .ok_or("incomplete PR closing references")?
            {
                if text(row, "/repository/nameWithOwner")? != repository {
                    return Err("cross-repository closing reference".to_owned());
                }
                if !issues.insert(positive(row, "/number")?) {
                    return Err("duplicate PR closing reference".to_owned());
                }
            }
            match connection["pageInfo"]["hasNextPage"].as_bool() {
                Some(false) => return Ok(issues.into_iter().collect()),
                Some(true) => {
                    let next = text(connection, "/pageInfo/endCursor")?;
                    if !seen.insert(next.to_owned()) || seen.len() > 1000 {
                        return Err("invalid closing-reference pagination".to_owned());
                    }
                    cursor = Value::String(next.to_owned());
                }
                None => return Err("incomplete closing-reference pagination".to_owned()),
            }
        }
    }

    fn closed_by(&self, repository: &str, number: u64) -> Result<Vec<u64>, String> {
        let (owner, name) = repository.split_once('/').ok_or("invalid repository")?;
        let mut cursor = Value::Null;
        let mut seen = BTreeSet::new();
        let mut events = BTreeSet::new();
        let mut merges = Vec::new();
        loop {
            let query = format!(
                "query {{ repository(owner: {owner:?}, name: {name:?}) {{ issue(number: {number}) {{ timelineItems(itemTypes: [CLOSED_EVENT], first: 100, after: {cursor}) {{ nodes {{ ... on ClosedEvent {{ id closer {{ __typename ... on PullRequest {{ number repository {{ nameWithOwner }} }} }} }} }} pageInfo {{ hasNextPage endCursor }} }} }} }} }}"
            );
            let result = self.client.graphql(&query)?;
            if result.get("errors").is_some() {
                return Err("incomplete issue closure evidence".to_owned());
            }
            let connection = result
                .pointer("/data/repository/issue/timelineItems")
                .ok_or("missing issue closure evidence")?;
            for event in connection["nodes"]
                .as_array()
                .ok_or("incomplete issue closure events")?
            {
                if !events.insert(text(event, "/id")?.to_owned()) {
                    return Err("duplicate issue closure event".to_owned());
                }
                let closer = event.get("closer").ok_or("missing closure origin")?;
                // A later closure replaces the old implementation binding.
                merges.clear();
                if closer.is_null() {
                    continue;
                }
                match text(closer, "/__typename")? {
                    "PullRequest" => {
                        if text(closer, "/repository/nameWithOwner")? != repository {
                            return Err("foreign issue closure".to_owned());
                        }
                        merges.push(positive(closer, "/number")?);
                    }
                    "Commit" => {}
                    _ => return Err("unknown issue closure origin".to_owned()),
                }
            }
            match connection["pageInfo"]["hasNextPage"].as_bool() {
                Some(false) => return Ok(merges),
                Some(true) => {
                    let next = text(connection, "/pageInfo/endCursor")?;
                    if !seen.insert(next.to_owned()) || seen.len() > 1000 {
                        return Err("invalid closure-event pagination".to_owned());
                    }
                    cursor = Value::String(next.to_owned());
                }
                None => return Err("incomplete closure-event pagination".to_owned()),
            }
        }
    }

    fn owner_ready(
        &self,
        prefix: &str,
        issue: &Value,
        owner: &str,
    ) -> Result<Option<OwnerReady>, String> {
        let number = positive(issue, "/number")?;
        let labels = issue["labels"].as_array().ok_or("missing issue labels")?;
        if !labels
            .iter()
            .any(|label| label["name"] == "ready-for-agent")
        {
            return Ok(None);
        }
        let events = self.list(&format!("{prefix}/issues/{number}/events"), None)?;
        let mut current = None;
        let mut previous_id = 0;
        for event in events {
            let id = positive(&event, "/id")?;
            if id <= previous_id {
                return Err("ambiguous issue event ordering".to_owned());
            }
            previous_id = id;
            if event["label"]["name"] != "ready-for-agent" {
                continue;
            }
            match text(&event, "/event")? {
                "labeled" => {
                    let actor = text(&event, "/actor/login")?;
                    current = (event["actor"]["type"] == "User"
                        && actor.eq_ignore_ascii_case(owner))
                    .then(|| OwnerReady {
                        repository: prefix.trim_start_matches("repos/").to_owned(),
                        issue: number,
                        applied_by: actor.to_owned(),
                        label_present: true,
                    });
                }
                "unlabeled" => current = None,
                _ => {}
            }
        }
        Ok(current)
    }

    fn work(
        &self,
        repository: &str,
        owner: &str,
        revision: &str,
        pulls: &[Value],
        links: &BTreeMap<u64, Vec<u64>>,
    ) -> Result<Vec<ActualWork>, String> {
        let prefix = format!("repos/{repository}");
        let mut work = BTreeMap::<u64, ActualWork>::new();
        let mut add = |issue: u64, copilot: bool| {
            let row = work.entry(issue).or_insert(ActualWork {
                issue,
                active_copilot: 0,
                open_implementation_prs: 0,
            });
            if copilot {
                row.active_copilot = row.active_copilot.saturating_add(1);
            } else {
                row.open_implementation_prs = row.open_implementation_prs.saturating_add(1);
            }
        };
        for pr in pulls.iter().filter(|pr| pr["state"] == "open") {
            let number = positive(pr, "/number")?;
            let issues = links.get(&number).ok_or("missing PR links")?;
            if issues.is_empty() {
                return Err("open PR has no authoritative task binding".to_owned());
            }
            for issue in issues {
                add(*issue, false);
            }
        }
        for issue in self.list(&format!("{prefix}/issues?state=all"), None)? {
            if issue.get("pull_request").is_some() {
                continue;
            }
            let number = positive(&issue, "/number")?;
            for assignee in issue["assignees"]
                .as_array()
                .ok_or("missing issue assignees")?
            {
                if is_copilot(text(assignee, "/login")?) {
                    if issue["state"] == "closed" {
                        let closures = self.closed_by(repository, number)?;
                        if let [pr] = closures.as_slice() {
                            self.human_merge(repository, *pr, owner, revision)?;
                            continue;
                        }
                    } else if issue["state"] != "open" {
                        return Err("unknown assigned issue state".to_owned());
                    }
                    add(number, true);
                }
            }
        }
        let mut task_ids = BTreeSet::new();
        for archived in [false, true] {
            let tasks = self.list(
                &format!("agents/repos/{repository}/tasks?is_archived={archived}"),
                Some("tasks"),
            )?;
            for task in tasks {
                if !task_ids.insert(text(&task, "/id")?.to_owned()) {
                    return Err("duplicate agent task across pages".to_owned());
                }
                match text(&task, "/state")? {
                    "completed" | "failed" | "timed_out" | "cancelled" => continue,
                    "queued" | "in_progress" | "idle" | "waiting_for_user" => {}
                    _ => return Err("unknown agent task state".to_owned()),
                }
                let artifacts = task["artifacts"]
                    .as_array()
                    .ok_or("missing agent task artifacts")?;
                let mut bound = BTreeSet::new();
                for artifact in artifacts {
                    if artifact["provider"] == "github" && artifact["type"] == "pull" {
                        let id = positive(artifact, "/data/id")?;
                        let pr = pulls
                            .iter()
                            .find(|pr| pr["id"] == id)
                            .ok_or("agent task references an unavailable PR")?;
                        let number = positive(pr, "/number")?;
                        let issues = match links.get(&number) {
                            Some(issues) => issues.clone(),
                            None => self.closing_issues(repository, number)?,
                        };
                        bound.extend(issues);
                    }
                }
                if bound.is_empty() {
                    return Err("active agent task has no authoritative issue binding".to_owned());
                }
                for issue in bound {
                    add(issue, true);
                }
            }
        }
        Ok(work.into_values().collect())
    }

    fn gates(&self, prefix: &str, config: &FactoryConfig) -> Result<KillSwitches, String> {
        let variables = self.list(&format!("{prefix}/actions/variables"), Some("variables"))?;
        let enabled = |name| {
            let matches: Vec<_> = variables.iter().filter(|row| row["name"] == name).collect();
            match matches.as_slice() {
                [row] if row["value"] == "true" => Evidence::Known(false),
                [row] if row["value"] == "false" => Evidence::Known(true),
                _ => Evidence::Unknown,
            }
        };
        let secrets = self.list(&format!("{prefix}/actions/secrets"), Some("secrets"))?;
        let secret = &config.modules.build_cloud.token_secret;
        let workflow = self.get(&format!("{prefix}/actions/workflows/build-cloud.lock.yml"))?;
        let workflow_active = match text(&workflow, "/state")? {
            "active" => true,
            "disabled_manually" | "disabled_inactivity" | "disabled_fork" | "deleted" => false,
            _ => return Err("unknown assignment workflow state".to_owned()),
        };
        let configured = !secret.is_empty()
            && secret == &config.engines.build_cloud.token_secret
            && secrets.iter().filter(|row| row["name"] == *secret).count() == 1;
        let assignment = if config.modules.build_cloud.enabled
            && config.modules.build_cloud.workflow_enabled
            && config.modules.build_cloud.assignment_enabled
            && workflow_active
            && config.modules.build_cloud.max_repositories == 1
            && config.modules.build_cloud.max_pull_requests > 0
            && config.modules.build_cloud.target_minutes > 0
            && !config.modules.build_cloud.staged
            && config.engines.build_cloud.engine == "copilot"
            && config.budget.enforce_ai_credits
            && config.budget.max_ai_credits_per_run > 0
            && configured
        {
            enabled("ACME_CLOUD_ASSIGNMENT_ENABLED")
        } else {
            Evidence::Known(true)
        };
        Ok(KillSwitches {
            global: enabled("ACME_AGENT_WORKFLOWS_ENABLED"),
            coordinator: if self.manual {
                Evidence::Known(false)
            } else {
                enabled("ACME_COORDINATOR_ENABLED")
            },
            assignment,
        })
    }

    fn acceptance(
        &self,
        repository: &str,
        issue: u64,
        merge: &str,
        owner: &str,
        revision: &str,
        paths: &BTreeSet<String>,
    ) -> Result<Option<AcceptanceReceipt>, String> {
        let path = format!("factory/dispatch/acceptance/{issue}.json");
        if !paths.contains(&path) {
            return Ok(None);
        }
        let content = self.client.file(repository, &path, revision)?;
        let receipt: ManualAcceptance =
            serde_json::from_str(&content).map_err(|_| "invalid acceptance receipt")?;
        if receipt.version != 1 || receipt.issue != issue || receipt.merge_commit != merge {
            return Err("acceptance receipt does not match implementation".to_owned());
        }
        let pr = self.file_approval(repository, &path, &content, owner, revision)?;
        let accepted_at = text(&pr, "/merge_commit_sha")?;
        self.ancestor(&format!("repos/{repository}"), merge, accepted_at)?;
        if merge == accepted_at {
            return Err("acceptance must follow implementation merge".to_owned());
        }
        Ok(Some(AcceptanceReceipt {
            issue,
            merge_commit: merge.to_owned(),
            accepted_by: owner.to_owned(),
            receipt_id: format!("{path}@{accepted_at}"),
        }))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualAcceptance {
    version: u32,
    issue: u64,
    merge_commit: String,
}

struct TaskRead<'a> {
    repository: &'a str,
    owner: &'a str,
    revision: &'a str,
    work: &'a [ActualWork],
    paths: &'a BTreeSet<String>,
}

struct Declarations {
    contracts: Vec<String>,
    tasks: BTreeMap<u64, DispatchDeclaration>,
    approvals: BTreeMap<u64, Result<Value, String>>,
}

impl<C: GitHubClient> GitHubStateReader<C> {
    fn paths(&self, prefix: &str, revision: &str) -> Result<BTreeSet<String>, String> {
        let tree = self.get(&format!("{prefix}/git/trees/{revision}?recursive=1"))?;
        if tree["truncated"] != false {
            return Err("protected-main tree is incomplete".to_owned());
        }
        let mut paths = BTreeSet::new();
        for entry in tree["tree"]
            .as_array()
            .ok_or("missing protected-main tree")?
        {
            let path = text(entry, "/path")?;
            if path.starts_with("factory/dispatch/") && entry["type"] != "tree" {
                if entry["type"] != "blob" || entry["mode"] != "100644" {
                    return Err("dispatch data must be regular files".to_owned());
                }
                if !paths.insert(path.to_owned()) {
                    return Err("duplicate protected-main path".to_owned());
                }
            }
        }
        Ok(paths)
    }

    fn declarations(
        &self,
        repository: &str,
        owner: &str,
        revision: &str,
        paths: &BTreeSet<String>,
    ) -> Result<Declarations, String> {
        let mut result = Declarations {
            contracts: Vec::new(),
            tasks: BTreeMap::new(),
            approvals: BTreeMap::new(),
        };
        for path in paths.iter().filter(|path| {
            std::path::Path::new(path)
                .extension()
                .is_some_and(|extension| extension == "toml")
        }) {
            if !self.manual && path.starts_with("factory/dispatch/manual/") {
                continue;
            }
            let content = self.client.file(repository, path, revision)?;
            let task =
                DispatchDeclaration::from_toml(&content).map_err(|error| error.to_string())?;
            if task.repository != repository || result.tasks.contains_key(&task.issue) {
                return Err("foreign or duplicate dispatch declaration".to_owned());
            }
            result.approvals.insert(
                task.issue,
                self.file_approval(repository, path, &content, owner, revision),
            );
            result.tasks.insert(task.issue, task);
            result.contracts.push(content);
        }
        Ok(result)
    }

    fn completion(
        &self,
        read: &TaskRead<'_>,
        number: u64,
        excluded: &BTreeSet<u64>,
        approved_revisions: &BTreeSet<String>,
    ) -> Result<Evidence<Option<PrerequisiteCompletion>>, String> {
        let mut completions = Vec::new();
        for pr_number in self.closed_by(read.repository, number)? {
            if !excluded.contains(&pr_number) {
                let merged =
                    self.human_merge(read.repository, pr_number, read.owner, read.revision)?;
                let merge = text(&merged, "/merge_commit_sha")?;
                for approved_revision in approved_revisions {
                    if approved_revision == merge {
                        return Err(
                            "implementation does not follow its approved snapshot".to_owned()
                        );
                    }
                    self.ancestor(
                        &format!("repos/{}", read.repository),
                        approved_revision,
                        merge,
                    )?;
                }
                completions.push(PrerequisiteCompletion {
                    repository: read.repository.to_owned(),
                    issue: number,
                    implementation_pull_request: pr_number,
                    merge_commit: merge.to_owned(),
                    merged_by: text(&merged, "/merged_by/login")?.to_owned(),
                    acceptance: self.acceptance(
                        read.repository,
                        number,
                        merge,
                        read.owner,
                        read.revision,
                        read.paths,
                    )?,
                });
            }
        }
        Ok(match completions.len() {
            0 => Evidence::Known(None),
            1 => Evidence::Known(completions.pop()),
            _ => Evidence::Unknown,
        })
    }

    fn observation(
        &self,
        read: &TaskRead<'_>,
        task: &DispatchDeclaration,
        live: &Value,
        approved: Option<Result<Value, String>>,
    ) -> Result<TaskObservation, String> {
        let number = task.issue;
        validate_issue(live, number)?;
        let spec = self
            .client
            .file(read.repository, &task.spec.path, read.revision);
        let spec_approval = spec.as_ref().map_err(Clone::clone).and_then(|content| {
            if content_sha256(content) != task.spec.sha256 {
                return Err("spec content changed".to_owned());
            }
            self.file_approval(
                read.repository,
                &task.spec.path,
                content,
                read.owner,
                read.revision,
            )
        });
        let approved = approved.unwrap_or_else(|| Err("no versioned approval".to_owned()));
        let excluded = [&approved, &spec_approval]
            .into_iter()
            .filter_map(|approval| approval.as_ref().ok())
            .map(|pr| positive(pr, "/number"))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let approved_revisions = [&approved, &spec_approval]
            .into_iter()
            .filter_map(|approval| approval.as_ref().ok())
            .map(|pr| text(pr, "/merge_commit_sha").map(str::to_owned))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let approval = match (approved, spec_approval) {
            (Ok(pr), Ok(_)) => Evidence::Known(Some(ApprovalEvidence {
                repository: read.repository.to_owned(),
                issue: number,
                spec_path: task.spec.path.clone(),
                spec_sha256: task.spec.sha256.clone(),
                issue_body_sha256: task.issue_body.sha256.clone(),
                kind: task.kind.clone(),
                prerequisites: task.prerequisites.clone(),
                spec_pull_request: positive(&pr, "/number")?,
                merged_by: text(&pr, "/merged_by/login")?.to_owned(),
                main_revision: read.revision.to_owned(),
                present_on_protected_main: true,
            })),
            _ => Evidence::Unknown,
        };
        let prefix = format!("repos/{}", read.repository);
        let native = self
            .list(
                &format!("{prefix}/issues/{number}/dependencies/blocked_by"),
                None,
            )?
            .iter()
            .map(|row| {
                if text(row, "/repository_url")? != format!("https://api.github.com/{prefix}") {
                    return Err("foreign native dependency".to_owned());
                }
                positive(row, "/number")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let completion = if live["state"] != "closed" {
            Evidence::Known(None)
        } else if matches!(approval, Evidence::Known(Some(_)))
            && content_sha256(text(live, "/body")?) == task.issue_body.sha256
        {
            self.completion(read, number, &excluded, &approved_revisions)?
        } else {
            Evidence::Unknown
        };
        let current = read.work.iter().find(|row| row.issue == number);
        Ok(TaskObservation {
            issue: number,
            current_spec: spec.map_or(Evidence::Error, Evidence::Known),
            current_issue_body: Evidence::Known(text(live, "/body")?.to_owned()),
            issue_state: Evidence::Known(if live["state"] == "open" {
                IssueState::Open
            } else {
                IssueState::Closed
            }),
            approval,
            native_prerequisites: Evidence::Known(native),
            existing_work: Evidence::Known(ExistingWork {
                active_claims: 0,
                active_assignments: current.map_or(0, |row| row.active_copilot),
                open_unmerged_implementation_prs: current
                    .map_or(0, |row| row.open_implementation_prs),
            }),
            completion,
        })
    }
}

impl<C: GitHubClient> GitHubStateReader<C> {
    fn pull_links(
        &self,
        repository: &str,
        pulls: &[Value],
    ) -> Result<BTreeMap<u64, Vec<u64>>, String> {
        let mut links = BTreeMap::new();
        let mut numbers = BTreeSet::new();
        for pr in pulls {
            let number = positive(pr, "/number")?;
            if !numbers.insert(number) {
                return Err("duplicate PR evidence".to_owned());
            }
            if pr["state"] == "open" {
                links.insert(number, self.closing_issues(repository, number)?);
            } else if pr["state"] != "closed" {
                return Err("unknown PR state".to_owned());
            }
        }
        Ok(links)
    }
}

impl<C: GitHubClient> GitHubState for GitHubStateReader<C> {
    fn observe(&self, request: &AssignmentRequest) -> Result<GitHubSnapshot, String> {
        let declaration = request.declaration();
        let repository = &declaration.repository;
        let prefix = format!("repos/{repository}");
        let identity = self.get("user")?;
        text(&identity, "/login")?;
        if identity["type"] != "User" {
            return Err("authenticated human-scoped API access is required".to_owned());
        }
        let repo = self.get(&prefix)?;
        if text(&repo, "/full_name")? != repository
            || text(&repo, "/default_branch")? != "main"
            || repo["owner"]["type"] != "User"
        {
            return Err("unsupported repository identity or owner policy".to_owned());
        }
        let owner = text(&repo, "/owner/login")?;
        if repository.split_once('/').map(|(login, _)| login) != Some(owner) {
            return Err("repository owner metadata is inconsistent".to_owned());
        }
        let revision = self.branch(&prefix)?;
        if revision != request.approved_main_revision() {
            return Err("request does not match current protected main".to_owned());
        }
        let paths = self.paths(&prefix, &revision)?;
        let config_text = self
            .client
            .file(repository, "factory/factory.toml", &revision)?;
        let config = FactoryConfig::from_toml(&config_text).map_err(|error| error.to_string())?;
        if config.repository != *repository {
            return Err("protected-main config targets a different repository".to_owned());
        }
        self.file_approval(
            repository,
            "factory/factory.toml",
            &config_text,
            owner,
            &revision,
        )?;
        let kill_switches = self.gates(&prefix, &config)?;
        let mut declarations = self.declarations(repository, owner, &revision, &paths)?;
        let issue = self.get(&format!("{prefix}/issues/{}", declaration.issue))?;
        validate_issue(&issue, declaration.issue)?;
        let owner_ready = self.owner_ready(&prefix, &issue, owner)?;
        if self.manual && !declarations.tasks.contains_key(&declaration.issue) {
            return Err(
                "manual dispatch requires an independently approved protected-main contract"
                    .to_owned(),
            );
        }
        let pulls = self.list(&format!("{prefix}/pulls?state=all"), None)?;
        let links = self.pull_links(repository, &pulls)?;
        let work = self.work(repository, owner, &revision, &pulls, &links)?;
        let mut observations = Vec::new();
        let read = TaskRead {
            repository,
            owner,
            revision: &revision,
            paths: &paths,
            work: &work,
        };
        for (number, task) in declarations.tasks {
            let live = if number == declaration.issue {
                issue.clone()
            } else {
                self.get(&format!("{prefix}/issues/{number}"))?
            };
            observations.push(self.observation(
                &read,
                &task,
                &live,
                declarations.approvals.remove(&number),
            )?);
        }
        if self.branch(&prefix)? != revision {
            return Err("protected main changed during observation".to_owned());
        }
        let mut coordinator = config.coordinator;
        if self.manual {
            coordinator.automatic_dispatch = false;
        }
        Ok(GitHubSnapshot {
            repository: repository.clone(),
            main_revision: revision,
            owner_login: owner.to_owned(),
            authorized_approvers: vec![owner.to_owned()],
            config: coordinator,
            kill_switches,
            contracts: declarations.contracts,
            observations,
            work: Evidence::Known(work),
            resolutions: Evidence::Unknown,
            owner_ready: Evidence::Known(owner_ready),
        })
    }
}

fn text<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("missing or invalid GitHub field {pointer}"))
}

fn positive(value: &Value, pointer: &str) -> Result<u64, String> {
    value
        .pointer(pointer)
        .and_then(Value::as_u64)
        .filter(|number| *number > 0)
        .ok_or_else(|| format!("missing or invalid GitHub field {pointer}"))
}

fn valid_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_issue(issue: &Value, number: u64) -> Result<(), String> {
    if issue["number"] != number
        || issue.get("pull_request").is_some()
        || !matches!(text(issue, "/state")?, "open" | "closed")
    {
        return Err("invalid issue identity or state".to_owned());
    }
    Ok(())
}

fn is_copilot(login: &str) -> bool {
    ["Copilot", "copilot-swe-agent", "copilot-swe-agent[bot]"]
        .iter()
        .any(|candidate| login.eq_ignore_ascii_case(candidate))
}
