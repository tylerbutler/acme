use super::sync::{SyncError, read_regular};
use crate::FactoryConfig;
use std::path::{Component, Path};
use toml::Value;

pub(super) fn validate_config(config: &FactoryConfig) -> Result<(), SyncError> {
    if config.toolchain.acme.bootstrap {
        return Err(error("init requires toolchain.acme.bootstrap = false"));
    }
    if config.autonomy != "L3" {
        return Err(error("init requires L3 autonomy"));
    }
    if config.coordinator.automatic_dispatch {
        return Err(error(
            "init requires coordinator.automatic_dispatch = false",
        ));
    }
    let modules = &config.modules;
    if modules.spec.enabled && modules.spec.implementation != "human" {
        return Err(error(
            "enabled spec module requires implementation = \"human\"",
        ));
    }
    if modules.intake.enabled
        || modules.build_cloud.enabled
        || modules.intake.workflow_enabled
        || modules.build_cloud.workflow_enabled
        || modules.build_cloud.assignment_enabled
        || config.engines.review.enabled
    {
        return Err(error(
            "init requires agent modules and workflow/assignment flags off, and review disabled",
        ));
    }
    Ok(())
}

pub(super) fn preflight(config: &FactoryConfig, root: &Path) -> Result<(), SyncError> {
    let mut locked = false;
    if config.modules.ci.enabled {
        for command in &config.modules.ci.commands {
            locked |= validate_command(command)?;
        }
    }
    for path in [".cargo/config", ".cargo/config.toml"] {
        if read_regular(&root.join(path))?.is_some() {
            return Err(error(
                "init does not support .cargo config overrides; review them outside init",
            ));
        }
    }
    validate_toolchain(root)?;
    validate_manifest(root)?;
    if locked {
        let lock = read_toml(root, "Cargo.lock")?;
        if lock.as_table().is_none_or(toml::Table::is_empty) {
            return Err(error(
                "Cargo.lock must be nonempty; generate it before init",
            ));
        }
    }
    Ok(())
}

// Only these Cargo forms have prerequisites that init can check locally.
fn validate_command(command: &str) -> Result<bool, SyncError> {
    let invalid = || {
        error(
            "supported CI commands: cargo fmt [--all] -- --check; cargo check/build/test/clippy with --locked, --all-targets, --all-features, --no-default-features; clippy may end with -- -D warnings. Project scripts, shell syntax and other flags need separate review",
        )
    };
    if command.chars().any(char::is_control) {
        return Err(invalid());
    }
    let words: Vec<_> = command.split(' ').filter(|word| !word.is_empty()).collect();
    let ["cargo", subcommand, flags @ ..] = words.as_slice() else {
        return Err(invalid());
    };
    if *subcommand == "fmt" {
        return if matches!(flags, ["--", "--check"] | ["--all", "--", "--check"]) {
            Ok(false)
        } else {
            Err(invalid())
        };
    }
    if !matches!(*subcommand, "check" | "build" | "test" | "clippy") {
        return Err(invalid());
    }
    let flags = if *subcommand == "clippy" {
        flags
            .strip_suffix(&["--", "-D", "warnings"])
            .unwrap_or(flags)
    } else {
        flags
    };
    if flags.iter().any(|flag| {
        !matches!(
            *flag,
            "--locked" | "--all-targets" | "--all-features" | "--no-default-features"
        )
    }) {
        return Err(invalid());
    }
    Ok(flags.contains(&"--locked"))
}

fn validate_toolchain(root: &Path) -> Result<(), SyncError> {
    let legacy = read_regular(&root.join("rust-toolchain"))?;
    let modern = read_regular(&root.join("rust-toolchain.toml"))?;
    if legacy.is_some() && modern.is_some() {
        return Err(error("use only one Rust toolchain input"));
    }
    if let Some(bytes) = legacy {
        let channel =
            std::str::from_utf8(&bytes).map_err(|_| error("rust-toolchain must be UTF-8"))?;
        if channel.trim().is_empty() {
            return Err(error("rust-toolchain must be nonempty"));
        }
    }
    if let Some(bytes) = modern {
        let toolchain = parse_toml(&bytes, "rust-toolchain.toml")?;
        let table = toolchain
            .get("toolchain")
            .and_then(Value::as_table)
            .ok_or_else(|| error("rust-toolchain.toml needs a toolchain table"))?;
        if table
            .get("channel")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(error(
                "rust-toolchain.toml needs a nonempty toolchain.channel",
            ));
        }
    }
    Ok(())
}

fn validate_manifest(root: &Path) -> Result<(), SyncError> {
    let manifest = read_toml(root, "Cargo.toml")?;
    if manifest.get("workspace").is_some() {
        return Err(error(
            "init supports single-package Cargo projects, not workspaces",
        ));
    }
    let package = manifest
        .get("package")
        .and_then(Value::as_table)
        .ok_or_else(|| error("Cargo.toml needs a package table"))?;
    for field in ["name", "version"] {
        if package
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(error(&format!(
                "Cargo.toml needs a nonempty package.{field}"
            )));
        }
    }
    if package.contains_key("workspace") {
        return Err(error(
            "init supports single-package projects without workspace inheritance",
        ));
    }
    validate_targets(root, &manifest, package)
}

fn validate_targets(root: &Path, manifest: &Value, package: &toml::Table) -> Result<(), SyncError> {
    let mut targets = 0;
    for (field, default) in [("lib", "src/lib.rs"), ("bin", "src/main.rs")] {
        if let Some(value) = manifest.get(field) {
            if field == "lib" {
                source_target(root, value, default)?;
                targets += 1;
            } else {
                for target in value
                    .as_array()
                    .ok_or_else(|| error("Cargo.toml bin must be an array"))?
                {
                    if target.get("path").is_none() {
                        return Err(error(
                            "init requires explicit paths for declared bin targets",
                        ));
                    }
                    source_target(root, target, default)?;
                    targets += 1;
                }
            }
        } else if package
            .get(if field == "lib" {
                "autolib"
            } else {
                "autobins"
            })
            .and_then(Value::as_bool)
            != Some(false)
            && read_regular(&root.join(default))?.is_some()
        {
            require_source(root, default)?;
            targets += 1;
        }
    }
    if targets == 0 {
        return Err(error(
            "Cargo.toml needs a Rust source target: src/main.rs, src/lib.rs or an explicit target path",
        ));
    }
    for field in ["example", "test", "bench"] {
        if let Some(value) = manifest.get(field) {
            for target in value
                .as_array()
                .ok_or_else(|| error("Cargo.toml targets must be arrays"))?
            {
                let path = target.get("path").and_then(Value::as_str).ok_or_else(|| {
                    error("init requires explicit paths for example/test/bench targets")
                })?;
                require_source(root, path)?;
            }
        }
    }
    match package.get("build") {
        Some(Value::String(path)) => require_source(root, path)?,
        Some(Value::Boolean(false)) => {}
        Some(_) => return Err(error("Cargo.toml package.build must be a path or false")),
        None => {
            if read_regular(&root.join("build.rs"))?.is_some() {
                require_source(root, "build.rs")?;
            }
        }
    }
    Ok(())
}

fn source_target(root: &Path, value: &Value, default: &str) -> Result<(), SyncError> {
    let table = value
        .as_table()
        .ok_or_else(|| error("Cargo.toml target must be a table"))?;
    let path = match table.get("path") {
        Some(path) => path
            .as_str()
            .ok_or_else(|| error("Cargo.toml target path must be a string"))?,
        None => default,
    };
    require_source(root, path)
}

fn require_source(root: &Path, path: &str) -> Result<(), SyncError> {
    if path.is_empty()
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(error(
            "Cargo.toml source path must be relative without parent components",
        ));
    }
    let bytes = required(root, path)?;
    if Path::new(path)
        .extension()
        .is_none_or(|extension| extension != "rs")
        || std::str::from_utf8(&bytes).is_err()
        || bytes.iter().all(u8::is_ascii_whitespace)
    {
        return Err(error(&format!(
            "Rust source must be nonempty UTF-8: {path}"
        )));
    }
    Ok(())
}

fn read_toml(root: &Path, path: &str) -> Result<Value, SyncError> {
    parse_toml(&required(root, path)?, path)
}

fn parse_toml(bytes: &[u8], path: &str) -> Result<Value, SyncError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| self::error(&format!("invalid {path}: {error}")))?;
    toml::from_str(text).map_err(|error| self::error(&format!("invalid {path}: {error}")))
}

fn required(root: &Path, path: &str) -> Result<Vec<u8>, SyncError> {
    read_regular(&root.join(path))?.ok_or_else(|| {
        error(&format!(
            "missing init prerequisite {path}; prepare the project before init"
        ))
    })
}

fn error(message: &str) -> SyncError {
    SyncError(message.to_owned())
}
