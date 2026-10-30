use super::{BytePlan, final_byte_plan, init, m2_byte_plan};
use crate::agent_catalog::agent_modules;
use crate::{FactoryConfig, builtin_modules};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

const RECORD: &str = "factory/managed-files.json";

#[derive(Debug)]
pub struct SyncError(pub(super) String);

impl fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SyncError {}

#[derive(Debug)]
pub struct SyncReport {
    /// All planned output paths, including exact-byte adoptions.
    pub outputs: Vec<String>,
    pub creates: Vec<String>,
    pub updates: Vec<String>,
    pub removals: Vec<String>,
    pub metadata_changed: bool,
}

impl SyncReport {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.creates.is_empty()
            && self.updates.is_empty()
            && self.removals.is_empty()
            && !self.metadata_changed
    }
}

impl fmt::Display for SyncReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let changes: BTreeMap<_, _> = self
            .creates
            .iter()
            .map(|path| (path.as_str(), "create"))
            .chain(self.updates.iter().map(|path| (path.as_str(), "update")))
            .chain(self.removals.iter().map(|path| (path.as_str(), "remove")))
            .collect();
        for (path, action) in changes {
            writeln!(formatter, "{action} {path}")?;
        }
        if self.metadata_changed {
            writeln!(formatter, "update {RECORD}")?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OwnershipRecord {
    format_version: u32,
    acme_version: String,
    entries: Vec<OwnedFile>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OwnedFile {
    module: String,
    path: String,
    sha256: String,
}

struct Change {
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

/// Check all paths before writes. Do not run with concurrent checkout writers.
///
/// Missing owned outputs are recreated. Missing obsolete outputs need no deletion.
/// Enabled agents require `sync_repository_with_compiled_inputs` instead.
pub fn sync_repository(
    config: &FactoryConfig,
    root: &Path,
    check: bool,
) -> Result<SyncReport, SyncError> {
    sync_repository_with_compiled_inputs(config, root, check, None)
}

/// Synchronize the full inventory from validated review bytes. Never compile.
///
/// `root` supplies authored prerequisites and output paths. The review directory
/// resolves against the process working directory, not `root`. Enabled agents
/// require inputs. Check mode reports drift without writes; I/O failures during
/// application may leave partial output changes with the old ownership record.
pub fn sync_repository_with_compiled_inputs(
    config: &FactoryConfig,
    root: &Path,
    check: bool,
    compiled_inputs: Option<&Path>,
) -> Result<SyncReport, SyncError> {
    let plan = final_byte_plan(config, root, compiled_inputs)
        .map_err(|error| SyncError(error.to_string()))?;
    synchronize(config, root, check, None, plan)
}

/// Initialize from explicitly approved bytes without replacing existing config.
///
/// Resolve relative config paths against the repository root.
/// I/O failures can leave partial changes. Init writes the record last.
pub fn init_repository(root: &Path, config_path: &Path) -> Result<SyncReport, SyncError> {
    let source = root.join(config_path);
    let path = std::path::absolute(&source).map_err(|error| io_error(&source, &error))?;
    if path.components().any(|part| part == Component::ParentDir) {
        return Err(SyncError(
            "config path must not contain parent (..) components".to_owned(),
        ));
    }
    let bytes = read_regular(&path)?
        .ok_or_else(|| SyncError(format!("missing approved config: {}", path.display())))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| SyncError(format!("approved config is not UTF-8: {error}")))?;
    let config = FactoryConfig::from_toml(text).map_err(|error| SyncError(error.to_string()))?;
    init::validate_config(&config)?;
    let plan = m2_byte_plan(&config).map_err(|error| SyncError(error.to_string()))?;
    synchronize(&config, root, false, Some(&bytes), plan)
}

fn synchronize(
    config: &FactoryConfig,
    root: &Path,
    check: bool,
    init_config: Option<&[u8]>,
    plan: BytePlan,
) -> Result<SyncReport, SyncError> {
    let root = std::path::absolute(root).map_err(|error| io_error(root, &error))?;
    if root.components().any(|part| part == Component::ParentDir) {
        return Err(SyncError(
            "sync root must not contain parent (..) components".to_owned(),
        ));
    }
    check_directories(&root)?;
    if !root.is_dir() {
        return Err(SyncError(
            "sync root must be an existing directory".to_owned(),
        ));
    }
    let record_path = root.join(RECORD);
    let mut changes = BTreeMap::new();
    if let Some(bytes) = init_config {
        prepare_init(config, &root, bytes, &mut changes)?;
    }
    let old_bytes = read_regular(&record_path)?;
    let old = read_record(old_bytes.as_deref())?;
    let mut desired = BTreeMap::new();
    let mut entries = Vec::new();
    for output in plan.outputs {
        let path = output.path.to_owned();
        entries.push(OwnedFile {
            module: output.module,
            path: path.clone(),
            sha256: hash(&output.contents),
        });
        desired.insert(path, output.contents);
    }
    for entry in old {
        let before = read_regular(&root.join(&entry.path))?;
        if let Some(bytes) = &before {
            if hash(bytes) != entry.sha256 {
                return Err(SyncError(format!("modified owned file: {}", entry.path)));
            }
        }
        let after = desired.remove(&entry.path);
        if before != after {
            changes.insert(entry.path, Change { before, after });
        }
    }
    for (path, after) in desired {
        let before = read_regular(&root.join(&path))?;
        if before.as_ref().is_some_and(|bytes| bytes != &after) {
            return Err(SyncError(format!("conflicting unowned file: {path}")));
        }
        if before.is_none() {
            changes.insert(
                path,
                Change {
                    before,
                    after: Some(after),
                },
            );
        }
    }
    let record = OwnershipRecord {
        format_version: 1,
        acme_version: plan.acme_version.to_owned(),
        entries,
    };
    let mut new_bytes = serde_json::to_vec_pretty(&record)
        .map_err(|error| SyncError(format!("cannot encode ownership record: {error}")))?;
    new_bytes.push(b'\n');
    let outputs = record
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    let mut report = SyncReport {
        outputs,
        creates: Vec::new(),
        updates: Vec::new(),
        removals: Vec::new(),
        metadata_changed: old_bytes.as_ref() != Some(&new_bytes),
    };
    for (path, change) in &changes {
        match (&change.before, &change.after) {
            (None, Some(_)) => report.creates.push(path.clone()),
            (Some(_), Some(_)) => report.updates.push(path.clone()),
            (Some(_), None) => report.removals.push(path.clone()),
            (None, None) => unreachable!("unchanged paths have no change"),
        }
    }
    if check {
        return Ok(report);
    }
    apply_changes(
        &root,
        &changes,
        old_bytes.as_deref(),
        report.metadata_changed.then_some(new_bytes.as_slice()),
    )?;
    Ok(report)
}

fn prepare_init(
    config: &FactoryConfig,
    root: &Path,
    bytes: &[u8],
    changes: &mut BTreeMap<String, Change>,
) -> Result<(), SyncError> {
    let record_path = root.join(RECORD);
    check_directories(record_path.parent().expect("record has a parent"))?;
    match fs::symlink_metadata(&record_path) {
        Ok(_) => {
            return Err(SyncError(
                "project already has an ownership record; use acme sync after review".to_owned(),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(&record_path, &error)),
    }
    init::preflight(config, root)?;
    let before = read_regular(&root.join("factory/factory.toml"))?;
    if before.as_deref().is_some_and(|existing| existing != bytes) {
        return Err(SyncError(
            "conflicting approved config: factory/factory.toml".to_owned(),
        ));
    }
    if before.is_none() {
        changes.insert(
            "factory/factory.toml".to_owned(),
            Change {
                before,
                after: Some(bytes.to_vec()),
            },
        );
    }
    Ok(())
}

fn apply_changes(
    root: &Path,
    changes: &BTreeMap<String, Change>,
    record_before: Option<&[u8]>,
    record_after: Option<&[u8]>,
) -> Result<(), SyncError> {
    let mut applied = 0;
    let result = (|| {
        for (relative, change) in changes {
            let path = root.join(relative);
            verify_snapshot(&path, change.before.as_deref())?;
            if let Some(bytes) = &change.after {
                staged_replace(&path, bytes)?;
            } else {
                fs::remove_file(&path).map_err(|error| io_error(&path, &error))?;
            }
            applied += 1;
        }
        if let Some(bytes) = record_after {
            let record_path = root.join(RECORD);
            verify_snapshot(&record_path, record_before)?;
            staged_replace(&record_path, bytes)?;
        }
        Ok(())
    })();
    result.map_err(|error: SyncError| {
        SyncError(format!(
            "{error}; sync stopped after {applied} output changes; partial application is possible; ownership record was not advanced"
        ))
    })
}

fn read_record(bytes: Option<&[u8]>) -> Result<Vec<OwnedFile>, SyncError> {
    let Some(bytes) = bytes else {
        return Ok(Vec::new());
    };
    let record: OwnershipRecord = serde_json::from_slice(bytes)
        .map_err(|error| SyncError(format!("malformed ownership record: {error}")))?;
    if record.format_version != 1 {
        return Err(SyncError(format!(
            "unsupported ownership record format version: {}",
            record.format_version
        )));
    }
    if record.acme_version.is_empty()
        || record
            .acme_version
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(SyncError(
            "invalid ownership record acme version".to_owned(),
        ));
    }
    let mut inventory = BTreeMap::new();
    for module in builtin_modules() {
        let manifest = module
            .manifest()
            .map_err(|error| SyncError(error.to_string()))?;
        for asset in module.assets {
            inventory.insert(asset.output_path, manifest.name.clone());
        }
    }
    for module in agent_modules() {
        for entry in module.ownership {
            inventory.insert(entry.path, entry.module.name().to_owned());
        }
    }
    let mut paths = BTreeSet::new();
    for entry in &record.entries {
        if inventory.get(entry.path.as_str()) != Some(&entry.module) {
            return Err(SyncError(format!(
                "ownership record path {:?} is outside module {:?}'s built-in inventory",
                entry.path, entry.module
            )));
        }
        if !paths.insert(&entry.path) {
            return Err(SyncError(format!(
                "duplicate ownership record entry: {}",
                entry.path
            )));
        }
        if entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(SyncError(format!(
                "invalid SHA256 in ownership record: {}",
                entry.path
            )));
        }
    }
    Ok(record.entries)
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn check_directories(path: &Path) -> Result<(), SyncError> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_symlink() => {
                return Err(SyncError(format!(
                    "symlink directory component: {}",
                    ancestor.display()
                )));
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(SyncError(format!(
                    "not a real directory: {}",
                    ancestor.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(ancestor, &error)),
        }
    }
    Ok(())
}

pub fn read_regular(path: &Path) -> Result<Option<Vec<u8>>, SyncError> {
    check_directories(path.parent().expect("sync file has a parent"))?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_symlink() => {
            Err(SyncError(format!("symlink file: {}", path.display())))
        }
        Ok(metadata) if metadata.is_file() => fs::read(path)
            .map(Some)
            .map_err(|error| io_error(path, &error)),
        Ok(_) => Err(SyncError(format!("not a regular file: {}", path.display()))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(path, &error)),
    }
}

fn verify_snapshot(path: &Path, expected: Option<&[u8]>) -> Result<(), SyncError> {
    if read_regular(path)?.as_deref() != expected {
        return Err(SyncError(format!(
            "file changed during sync: {}",
            path.display()
        )));
    }
    Ok(())
}

fn staged_replace(path: &Path, bytes: &[u8]) -> Result<(), SyncError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().expect("sync file has a parent");
    let mut directory = std::path::PathBuf::new();
    for component in parent.components() {
        directory.push(component);
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                check_directories(&directory)?;
            }
            Err(error) => return Err(io_error(&directory, &error)),
        }
    }
    for _ in 0..128 {
        let temporary = parent.join(format!(
            ".acme-sync-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(&temporary, &error)),
        };
        let result = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        let result = result
            .and_then(|()| fs::rename(&temporary, path))
            .map_err(|error| io_error(path, &error));
        if let Err(error) = result {
            if let Err(cleanup) = fs::remove_file(&temporary) {
                return Err(SyncError(format!(
                    "{error}; cannot remove staged file {}: {cleanup}",
                    temporary.display()
                )));
            }
            return Err(error);
        }
        return Ok(());
    }
    Err(SyncError(format!(
        "cannot reserve a staged file for {}",
        path.display()
    )))
}

fn io_error(path: &Path, error: &io::Error) -> SyncError {
    SyncError(format!("sync I/O failure at {}: {error}", path.display()))
}
