//! Explicit, offline compilation and the reusable review-directory trust boundary.
use super::{AgentSourcePlan, build_agent_source_plan};
use crate::FactoryConfig;
use crate::agent_catalog::{AgentOwnedPath, AgentPathKind, agent_modules};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const RECORD: &str = "compiled-workflows.json";
const COMPILER: &str = "v0.93.5";
const ACTION_SHA: &str = "de9d416cac8ccea1a2d582cb07c4b7db96946792";
const ACTIONLINT_MINIMUM: (u32, u32, u32) = (1, 7, 12);
// Preserve the reviewed version comments and manifest.
// Resolve the tag only from the fixed local cache.
const OPTIONS: &[&str] = &[
    "--no-check-update",
    "--strict",
    "--action-mode",
    "action",
    "--action-tag",
    COMPILER,
];
const LINT_OPTIONS: &[&str] = &["-shellcheck=", "-pyflakes="];
const CACHE_PATH: &str = ".github/aw/actions-lock.json";
// Reviewed v0.93.5 actions. Never let a user cache select or resolve these refs.
const PINS: &[(&str, &str, &str)] = &[
    (
        "actions/checkout",
        "v7.0.1",
        "3d3c42e5aac5ba805825da76410c181273ba90b1",
    ),
    (
        "actions/download-artifact",
        "v8.0.2",
        "9000827ccba6bdab643e8b6fd33ac0654aef8333",
    ),
    (
        "actions/github-script",
        "v9.0.0",
        "3a2844b7e9c422d3c10d287c895573f7108da1b3",
    ),
    (
        "actions/setup-node",
        "v7.1.0",
        "949feb2413d6458794dcd2491c4babbbce0c15c1",
    ),
    (
        "actions/upload-artifact",
        "v7.0.2",
        "cf430e030ddbb5b0abf93d22962f4752f3646cd9",
    ),
    ("github/gh-aw-actions/setup", COMPILER, ACTION_SHA),
];

#[derive(Debug)]
pub struct CompileError(String);

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CompileError {}

/// Validated, independent bytes: consumers need not reopen the review directory.
#[derive(Debug, Eq, PartialEq)]
pub struct ValidatedCompiledInputs {
    pub outputs: Vec<CompiledAgentOutput>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct CompiledAgentOutput {
    pub ownership: AgentOwnedPath,
    pub contents: Vec<u8>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CompilationRecord {
    format_version: u32,
    acme_version: String,
    config_sha256: String,
    assets: BTreeMap<String, String>,
    compiler_version: String,
    compiler_options: Vec<String>,
    action_cache_sha256: String,
    validator_options: Vec<String>,
    outputs: Vec<RecordedOutput>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RecordedOutput {
    module: String,
    path: String,
    sha256: String,
}

/// Compile bundled sources without running repository commands or accessing credentials.
///
/// The compiler and actionlint must already be installed. The destination must be
/// new or empty. I/O failures may leave incomplete output, never a completed record.
/// As with sync, callers must exclude concurrent filesystem writers.
pub fn compile_agent_workflows(
    config: &FactoryConfig,
    repository: &Path,
    destination: &Path,
) -> Result<ValidatedCompiledInputs, CompileError> {
    let plan = source_plan(config, repository)?;
    let destination = safe_absolute(destination)?;
    require_empty(&destination)?;
    let workspace = Workspace::new(repository, &destination)?;
    let root = workspace.0.join("repository");
    let home = workspace.0.join("home");
    create_directories(&root)?;
    create_directories(&home)?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let compiler = compiler_executable(&path)?;
    let actionlint = find_executable("actionlint", &path)
        .ok_or_else(|| error("compile requires already-installed actionlint on PATH; no tools are installed automatically"))?;
    let environment = Environment { path, home };
    let version = environment.run(&compiler, &["version"], &root)?;
    if !version.status.success()
        || String::from_utf8_lossy(&version.stdout).trim() != format!("gh aw version {COMPILER}")
    {
        return Err(error(format!(
            "compile requires gh-aw {COMPILER}; installed compiler reported {:?}",
            String::from_utf8_lossy(&version.stdout).trim()
        )));
    }
    let version = environment.run(&actionlint, &["-version"], &root)?;
    if !version.status.success() || !supported_actionlint(&version.stdout) {
        return Err(error(
            "compile requires native actionlint >=1.7.12 with a parseable stable version; no Docker fallback or installation is attempted",
        ));
    }
    for output in &plan.outputs {
        write_new(&root.join(output.path), &output.contents)?;
    }
    write_new(&root.join(CACHE_PATH), &action_cache()?)?;
    let mut arguments = vec!["compile"];
    arguments.extend_from_slice(OPTIONS);
    for output in &plan.outputs {
        if plan
            .ownership
            .iter()
            .any(|entry| entry.path == output.path && entry.kind == AgentPathKind::WorkflowSource)
        {
            arguments.push(output.path);
        }
    }
    if !plan.outputs.is_empty() {
        require_success(
            &environment.run(&compiler, &arguments, &root)?,
            "gh-aw compile",
        )?;
    }
    let mut expected = expected_paths(&plan);
    expected.insert(CACHE_PATH.to_owned());
    // gh-aw writes this infrastructure file; it is not part of module ownership.
    // Its bytes are compiler infrastructure, never handed off to a checkout.
    if root.join(".gitattributes").try_exists().map_err(io_error)? {
        expected.insert(".gitattributes".to_owned());
    }
    check_inventory(&root, &expected)?;
    let outputs = read_outputs(&plan, &root)?;
    let mut lint_arguments = LINT_OPTIONS.to_vec();
    lint_arguments.extend(
        plan.ownership
            .iter()
            .filter(|entry| entry.kind == AgentPathKind::CompilerOutput)
            .map(|entry| entry.path),
    );
    if lint_arguments.len() > LINT_OPTIONS.len() {
        require_success(
            &environment.run(&actionlint, &lint_arguments, &root)?,
            "actionlint",
        )?;
    }
    // Validators must not be able to mutate planned inputs after their initial read.
    check_inventory(&root, &expected)?;
    if read_outputs(&plan, &root)? != outputs {
        return Err(error("validation tools mutated compiled outputs"));
    }
    let record = make_record(config, &plan, &outputs)?;
    let record_bytes = json(&record)?;
    require_empty(&destination)?;
    create_directories(&destination)?;
    for output in &outputs {
        write_new(&destination.join(output.ownership.path), &output.contents)?;
    }
    check_inventory(&destination, &expected_paths(&plan))?;
    // Last write is the completion marker, only after the inventory is validated.
    write_new(&destination.join(RECORD), &record_bytes)?;
    validate_compiled_inputs(config, repository, &destination)
}

/// Reject stale, edited, extra or unsafe review inputs without invoking any tools.
///
/// Hashes detect mismatches, not authenticity. A directory is not a trusted
/// compiler attestation; a human still reviews the generated bytes.
pub fn validate_compiled_inputs(
    config: &FactoryConfig,
    repository: &Path,
    directory: &Path,
) -> Result<ValidatedCompiledInputs, CompileError> {
    let plan = source_plan(config, repository)?;
    let directory = safe_absolute(directory)?;
    let mut expected = expected_paths(&plan);
    expected.insert(RECORD.to_owned());
    check_inventory(&directory, &expected)?;
    let record: CompilationRecord = serde_json::from_slice(&read_regular(&directory.join(RECORD))?)
        .map_err(|source| error(format!("invalid {RECORD}: {source}")))?;
    let outputs = read_outputs(&plan, &directory)?;
    if record != make_record(config, &plan, &outputs)? {
        return Err(error(
            "compiled inputs are stale, edited or incompatible; explicitly recompile approved config",
        ));
    }
    Ok(ValidatedCompiledInputs { outputs })
}

fn source_plan(config: &FactoryConfig, repository: &Path) -> Result<AgentSourcePlan, CompileError> {
    if config.toolchain.gh_aw.version != COMPILER {
        return Err(error(format!(
            "compile requires toolchain.gh_aw.version = {COMPILER}"
        )));
    }
    let repository = safe_absolute(repository)?;
    check_directories(&repository)?;
    build_agent_source_plan(config, repository).map_err(|source| error(source.to_string()))
}

fn make_record(
    config: &FactoryConfig,
    plan: &AgentSourcePlan,
    outputs: &[CompiledAgentOutput],
) -> Result<CompilationRecord, CompileError> {
    let mut assets = BTreeMap::new();
    // Include capability-provider manifests too: their dependency declarations
    // participate in validating the source plan even though M2 is not compiled.
    for module in crate::builtin_modules() {
        let manifest = module
            .manifest()
            .map_err(|source| error(source.to_string()))?;
        assets.insert(
            format!("module:{}", manifest.name),
            hash(module.manifest_toml.as_bytes()),
        );
        for asset in module.assets {
            assets.insert(asset.output_path.to_owned(), hash(asset.contents));
            if asset.template() != asset.contents {
                assets.insert(
                    format!("template:{}", asset.output_path),
                    hash(asset.template()),
                );
            }
        }
    }
    for module in agent_modules() {
        assets.insert(
            format!("module:{}", module.module.name()),
            hash(module.manifest_toml.as_bytes()),
        );
        for asset in module.assets {
            assets.insert(asset.output_path.to_owned(), hash(asset.contents));
        }
    }
    Ok(CompilationRecord {
        format_version: 1,
        acme_version: plan.acme_version.to_owned(),
        config_sha256: hash(&json(config)?),
        assets,
        compiler_version: COMPILER.to_owned(),
        compiler_options: OPTIONS.iter().map(|value| (*value).to_owned()).collect(),
        action_cache_sha256: hash(&action_cache()?),
        validator_options: LINT_OPTIONS
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        outputs: outputs
            .iter()
            .map(|output| RecordedOutput {
                module: output.ownership.module.name().to_owned(),
                path: output.ownership.path.to_owned(),
                sha256: hash(&output.contents),
            })
            .collect(),
    })
}

fn expected_paths(plan: &AgentSourcePlan) -> BTreeSet<String> {
    plan.ownership
        .iter()
        .map(|entry| entry.path.to_owned())
        .collect()
}

fn read_outputs(
    plan: &AgentSourcePlan,
    directory: &Path,
) -> Result<Vec<CompiledAgentOutput>, CompileError> {
    plan.ownership
        .iter()
        .map(|ownership| {
            let contents = read_regular(&directory.join(ownership.path))?;
            if contents.is_empty() {
                return Err(error(format!("empty compiled input: {}", ownership.path)));
            }
            if let Some(source) = plan
                .outputs
                .iter()
                .find(|source| source.path == ownership.path)
            {
                if contents != source.contents {
                    return Err(error(format!(
                        "mutated expected agent source: {}",
                        ownership.path
                    )));
                }
            }
            Ok(CompiledAgentOutput {
                ownership: *ownership,
                contents,
            })
        })
        .collect()
}

fn action_cache() -> Result<Vec<u8>, CompileError> {
    let entries: BTreeMap<_, _> = PINS
        .iter()
        .map(|(repo, version, sha)| {
            (
                format!("{repo}@{version}"),
                serde_json::json!({"repo": repo, "version": version, "sha": sha}),
            )
        })
        .collect();
    json(&serde_json::json!({"entries": entries}))
}

struct Environment {
    path: OsString,
    home: PathBuf,
}

impl Environment {
    fn run(&self, executable: &Path, args: &[&str], root: &Path) -> Result<Output, CompileError> {
        Command::new(executable)
            .args(args)
            .current_dir(root)
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("GH_CONFIG_DIR", &self.home)
            .env("XDG_CONFIG_HOME", &self.home)
            .env("XDG_CACHE_HOME", &self.home)
            .env("XDG_DATA_HOME", &self.home)
            .env("TMPDIR", &self.home)
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .env("GH_NO_EXTENSION_UPDATE_NOTIFIER", "1")
            // gh-aw uses the GitHub API if a pin is missing from the cache.
            // Block unexpected HTTP(S) requests.
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .env("ALL_PROXY", "http://127.0.0.1:9")
            .env("http_proxy", "http://127.0.0.1:9")
            .env("https_proxy", "http://127.0.0.1:9")
            .env("all_proxy", "http://127.0.0.1:9")
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .stdin(Stdio::null())
            .output()
            .map_err(|source| error(format!("cannot run {}: {source}", executable.display())))
    }
}

fn supported_actionlint(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let Some(version) = text.split_whitespace().next() else {
        return false;
    };
    let mut parts = version.strip_prefix('v').unwrap_or(version).split('.');
    let mut next = || parts.next().and_then(|part| part.parse::<u32>().ok());
    let (Some(major), Some(minor), Some(patch)) = (next(), next(), next()) else {
        return false;
    };
    parts.next().is_none() && (major, minor, patch) >= ACTIONLINT_MINIMUM
}

fn compiler_executable(path: &std::ffi::OsStr) -> Result<PathBuf, CompileError> {
    if let Some(executable) = find_executable("gh-aw", path) {
        return Ok(executable);
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    if let Some(data) = data {
        let executable = data.join("gh/extensions/gh-aw/gh-aw");
        if executable.is_file() {
            return std::path::absolute(executable).map_err(io_error);
        }
    }
    Err(error(format!(
        "compile requires already-installed gh-aw {COMPILER} on PATH or in the standard gh extension directory; no installation is attempted"
    )))
}

fn find_executable(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| std::path::absolute(candidate).ok())
}

fn require_success(output: &Output, tool: &str) -> Result<(), CompileError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(error(format!(
            "{tool} failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

fn safe_absolute(path: &Path) -> Result<PathBuf, CompileError> {
    if path.components().any(|part| part == Component::ParentDir) {
        return Err(error(
            "compile paths must not contain parent (..) components",
        ));
    }
    let path = std::path::absolute(path).map_err(io_error)?;
    check_directories(&path)?;
    Ok(path)
}

fn check_directories(path: &Path) -> Result<(), CompileError> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {}
            Ok(_) => {
                return Err(error(format!(
                    "not a real directory: {}",
                    ancestor.display()
                )));
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(io_error(source)),
        }
    }
    Ok(())
}

fn require_empty(path: &Path) -> Result<(), CompileError> {
    check_directories(path)?;
    match fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().transpose().map_err(io_error)?.is_some() {
                return Err(error(format!(
                    "refusing nonempty compile destination: {}",
                    path.display()
                )));
            }
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(io_error(source)),
    }
    Ok(())
}

fn create_directories(path: &Path) -> Result<(), CompileError> {
    check_directories(path)?;
    fs::create_dir_all(path).map_err(io_error)
}

fn read_regular(path: &Path) -> Result<Vec<u8>, CompileError> {
    check_directories(path.parent().ok_or_else(|| error("input has no parent"))?)?;
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if !metadata.file_type().is_file() {
        return Err(error(format!(
            "not a regular compiled input: {}",
            path.display()
        )));
    }
    fs::read(path).map_err(io_error)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), CompileError> {
    create_directories(path.parent().ok_or_else(|| error("output has no parent"))?)?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(io_error)
}

fn check_inventory(root: &Path, expected: &BTreeSet<String>) -> Result<(), CompileError> {
    fn visit(
        root: &Path,
        directory: &Path,
        expected: &BTreeSet<String>,
        found: &mut BTreeSet<String>,
    ) -> Result<(), CompileError> {
        for entry in fs::read_dir(directory).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|source| error(source.to_string()))?;
            let relative = relative
                .to_str()
                .ok_or_else(|| error("non-UTF-8 compiled path"))?;
            let kind = entry.file_type().map_err(io_error)?;
            if kind.is_dir() {
                if !expected
                    .iter()
                    .any(|file| file.starts_with(&format!("{relative}/")))
                {
                    return Err(error(format!("extra compiled directory: {relative}")));
                }
                visit(root, &path, expected, found)?;
            } else if kind.is_file() && expected.contains(relative) {
                found.insert(relative.to_owned());
            } else {
                return Err(error(format!(
                    "extra or nonregular compiled input: {relative}"
                )));
            }
        }
        Ok(())
    }
    check_directories(root)?;
    let mut found = BTreeSet::new();
    visit(root, root, expected, &mut found)?;
    if &found != expected {
        return Err(error("missing files in compiled inventory"));
    }
    Ok(())
}

struct Workspace(PathBuf);

impl Workspace {
    fn new(repository: &Path, destination: &Path) -> Result<Self, CompileError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = safe_absolute(&std::env::temp_dir())?;
        let repository = safe_absolute(repository)?;
        if temporary.starts_with(&repository) || temporary.starts_with(destination) {
            return Err(error(
                "temporary directory must be outside the repository and review destination",
            ));
        }
        // gh-aw discovers Git roots by walking filesystem ancestors rather than
        // honoring Git's environment ceiling. An unrelated checkout around
        // TMPDIR would otherwise supply compiler settings/cache and receive
        // .gitattributes writes. Worktree marker files count too; do not read them.
        for ancestor in temporary.ancestors() {
            let marker = ancestor.join(".git");
            match fs::symlink_metadata(&marker) {
                Ok(_) => {
                    return Err(error(format!(
                        "temporary directory must not be inside any Git checkout: {}",
                        ancestor.display()
                    )));
                }
                Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(io_error(source)),
            }
        }
        for _ in 0..100 {
            let path = temporary.join(format!(
                "acme-compile-private-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            // Cleanup owns this whole candidate. Never place caller-owned review
            // output inside it, even when that future path does not exist yet.
            if destination.starts_with(&path) || path.starts_with(destination) {
                continue;
            }
            let mut builder = DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(io_error(source)),
            }
        }
        Err(error("cannot allocate private compilation workspace"))
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn json(value: &impl Serialize) -> Result<Vec<u8>, CompileError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| error(source.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn error(message: impl Into<String>) -> CompileError {
    CompileError(message.into())
}

fn io_error(source: impl fmt::Display) -> CompileError {
    error(format!(
        "compile I/O failure: {source}; output may be incomplete"
    ))
}

#[cfg(test)]
mod tests {
    use super::supported_actionlint;

    #[test]
    fn actionlint_requires_a_parseable_stable_minimum_version() {
        for version in ["v1.7.12\n", "1.7.12\n", "v1.8.0\n", "v2.0.0\n"] {
            assert!(supported_actionlint(version.as_bytes()), "{version:?}");
        }
        for version in [
            "",
            "unknown",
            "v1.7.11\n",
            "v0.9.99\n",
            "vv1.7.12\n",
            "v1.7\n",
            "v1.7.12.1\n",
            "v1.7.12-beta\n",
            "v999999999999999999999.0.0\n",
        ] {
            assert!(!supported_actionlint(version.as_bytes()), "{version:?}");
        }
        assert!(!supported_actionlint(&[0xff]));
    }
}
