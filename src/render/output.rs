use super::{PlannedOutput, TemplateInputs, build_render_plan};
use crate::FactoryConfig;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct RenderError(String);

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RenderError {}

pub fn render_to_directory(config: &FactoryConfig, destination: &Path) -> Result<(), RenderError> {
    let plan = build_render_plan(config).map_err(|error| RenderError(error.to_string()))?;
    let outputs = plan
        .outputs
        .iter()
        .map(|output| render_output(output).map(|bytes| (output.path, bytes)))
        .collect::<Result<Vec<_>, _>>()?;
    let destination =
        std::path::absolute(destination).map_err(|error| io_error(destination, &error))?;
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
    for (relative, bytes) in outputs {
        let path = destination.join(relative);
        create_directories(path.parent().expect("planned output has a parent"))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| io_error(&path, &error))?;
        file.write_all(&bytes)
            .map_err(|error| io_error(&path, &error))?;
    }
    Ok(())
}

fn render_output(output: &PlannedOutput) -> Result<Vec<u8>, RenderError> {
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
        && !value
            .chars()
            .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
    {
        value.to_owned()
    } else {
        json_string(value)
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value)
        .expect("serialize string")
        .replace('\u{85}', "\\u0085")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
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
