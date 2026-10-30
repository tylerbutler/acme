use super::{PlannedOutput, TemplateInputs, final_byte_plan};
use crate::FactoryConfig;
use std::fmt::{self, Write as _};
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub struct RenderError(String);

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RenderError {}

/// Render M2-only configurations; enabled agents require explicit compiled inputs.
pub fn render_to_directory(config: &FactoryConfig, destination: &Path) -> Result<(), RenderError> {
    render_to_directory_with_compiled_inputs(config, Path::new("."), destination, None)
}

/// Render the full inventory from captured, validated review bytes. Never compile.
///
/// `repository` supplies authored prerequisites. Paths resolve against the process
/// working directory. Enabled agents require `compiled_inputs`; supplied inputs
/// are always validated before creating the destination.
pub fn render_to_directory_with_compiled_inputs(
    config: &FactoryConfig,
    repository: &Path,
    destination: &Path,
    compiled_inputs: Option<&Path>,
) -> Result<(), RenderError> {
    let plan = final_byte_plan(config, repository, compiled_inputs)
        .map_err(|error| RenderError(error.to_string()))?;
    let destination =
        std::path::absolute(destination).map_err(|error| io_error(destination, &error))?;
    if destination
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(RenderError(
            "render destination must not contain parent (..) components".to_owned(),
        ));
    }
    check_ancestors(&destination)?;
    match fs::read_dir(&destination) {
        Ok(mut entries) => {
            if let Some(entry) = entries.next() {
                entry.map_err(|error| io_error(&destination, &error))?;
                return Err(RenderError(format!(
                    "refusing nonempty render destination {}; use sync to update a project",
                    destination.display()
                )));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(&destination, &error)),
    }

    create_directories(&destination)?;
    for output in plan.outputs {
        let path = destination.join(output.path);
        create_directories(path.parent().expect("planned output has a parent"))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| io_error(&path, &error))?;
        file.write_all(&output.contents)
            .map_err(|error| io_error(&path, &error))?;
    }
    Ok(())
}

pub(super) fn render_output(output: &PlannedOutput) -> Result<Vec<u8>, RenderError> {
    let substitutions = match &output.inputs {
        TemplateInputs::Static => Vec::new(),
        TemplateInputs::Ci {
            required_check,
            steps,
        } => {
            let rendered_steps = steps
                .iter()
                .map(|step| {
                    format!(
                        "      - name: {}\n        run: {}",
                        yaml_scalar(&step.name),
                        yaml_scalar(&step.command)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            vec![
                ("CI_CHECK", yaml_scalar(required_check)),
                ("CI_STEPS", rendered_steps),
            ]
        }
        TemplateInputs::Ruleset {
            required_checks,
            required_approvals,
            require_extra_approval_for_unattributed_changes,
        } => {
            let checks = required_checks
                .iter()
                .map(|check| {
                    format!(
                        "          {{\"context\": {}, \"integration_id\": 15368}}",
                        json_string(check)
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n");
            vec![
                ("REQUIRED_CHECKS", checks),
                ("REQUIRED_APPROVALS", required_approvals.to_string()),
                (
                    "EXTRA_APPROVAL",
                    require_extra_approval_for_unattributed_changes.to_string(),
                ),
            ]
        }
    };
    let template = std::str::from_utf8(output.template)
        .map_err(|error| RenderError(format!("invalid template for {}: {error}", output.path)))?;
    let template = template.replace("\r\n", "\n").replace('\r', "\n");
    let mut remaining = template.as_str();
    let mut rendered = String::new();
    while let Some((before, after)) = remaining.split_once("@@ACME_") {
        rendered.push_str(before);
        let (key, rest) = after.split_once("@@").ok_or_else(|| {
            RenderError(format!(
                "unclosed placeholder in template for {}",
                output.path
            ))
        })?;
        let (_, value) = substitutions
            .iter()
            .find(|(name, _)| *name == key)
            .ok_or_else(|| RenderError(format!("unknown placeholder {key} in {}", output.path)))?;
        rendered.push_str(value);
        remaining = rest;
    }
    rendered.push_str(remaining);
    Ok(rendered.into_bytes())
}

fn yaml_scalar(value: &str) -> String {
    let reserved = ["true", "false", "null", "yes", "no", "on", "off", "y", "n"];
    if value.starts_with(|character: char| character.is_ascii_alphabetic())
        && value.trim() == value
        && !reserved.contains(&value.to_ascii_lowercase().as_str())
        && !value.contains([':', '#', '\\', '@', '`', '{', '}', '[', ']'])
        && !value.chars().any(needs_unicode_escape)
    {
        value.to_owned()
    } else {
        json_string(value)
    }
}

fn json_string(value: &str) -> String {
    let mut escaped = String::new();
    for character in serde_json::to_string(value)
        .expect("serialize string")
        .chars()
    {
        if needs_unicode_escape(character) {
            write!(escaped, "\\u{:04x}", u32::from(character)).expect("write to string");
        } else {
            escaped.push(character);
        }
    }
    escaped
}

fn needs_unicode_escape(character: char) -> bool {
    character.is_control() || matches!(character, '\u{2028}' | '\u{2029}' | '\u{fffe}' | '\u{ffff}')
}

fn check_ancestors(path: &Path) -> Result<(), RenderError> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => {}
            Ok(_) => {
                return Err(RenderError(format!(
                    "render destination component is not a real directory: {}",
                    ancestor.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(ancestor, &error)),
        }
    }
    Ok(())
}

fn create_directories(path: &Path) -> Result<(), RenderError> {
    let mut directory = PathBuf::new();
    for component in path.components() {
        directory.push(component);
        match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&directory)
                    .map_err(|error| io_error(&directory, &error))?;
                if !metadata.is_dir() || metadata.is_symlink() {
                    return Err(RenderError(format!(
                        "render destination component is not a real directory: {}",
                        directory.display()
                    )));
                }
            }
            Err(error) => return Err(io_error(&directory, &error)),
        }
    }
    Ok(())
}

fn io_error(path: &Path, error: &io::Error) -> RenderError {
    RenderError(format!(
        "render I/O failure at {}: {error}; output may be incomplete",
        path.display()
    ))
}
