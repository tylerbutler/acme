use crate::FactoryConfig;
use crate::render::read_regular;
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinPreset {
    RustCli,
    RustLibrary,
}

pub fn select_builtin_preset(
    profile: &str,
    languages: &[String],
) -> Result<BuiltinPreset, &'static str> {
    let mut seen = BTreeSet::new();
    for language in languages {
        if !seen.insert(language.as_str()) {
            return Err("preset languages must not contain duplicates");
        }
    }
    if languages.len() != 1 || languages[0] != "rust" {
        return Err("supported preset combinations require exactly one rust language");
    }
    match profile {
        "cli" => Ok(BuiltinPreset::RustCli),
        "library" => Ok(BuiltinPreset::RustLibrary),
        _ => Err("unsupported preset profile"),
    }
}

const LIBRARY_REQUIRED_CHECK: &str = "CI";
const LIBRARY_CI_COMMANDS: [&str; 4] = [
    "cargo fmt --all -- --check",
    "cargo clippy --locked --all-targets --all-features -- -D warnings",
    "cargo test --locked --all-features",
    "cargo doc --locked --no-deps --all-features",
];

// Slice A keeps library preset checks internal. Public commands will use these checks in slice B.
#[allow(dead_code)]
pub fn validate_rust_library_config(config: &FactoryConfig) -> Result<(), String> {
    match select_builtin_preset(&config.profile, &config.languages) {
        Ok(BuiltinPreset::RustLibrary) => {}
        _ => {
            return Err(
                "unsupported library preset: require profile library and languages [rust]"
                    .to_owned(),
            );
        }
    }
    if config.autonomy != "L3" {
        return Err("library preset requires autonomy = \"L3\"".to_owned());
    }
    if config.coordinator.automatic_dispatch {
        return Err("library preset requires coordinator.automatic_dispatch = false".to_owned());
    }
    for (name, enabled) in [
        ("review", config.modules.review.enabled),
        ("holdout", config.modules.holdout.enabled),
        ("build_local", config.modules.build_local.enabled),
        ("release", config.modules.release.enabled),
        ("ops", config.modules.ops.enabled),
    ] {
        if enabled {
            return Err(format!(
                "library preset requires modules.{name}.enabled = false"
            ));
        }
    }
    if config.engines.review.enabled {
        return Err("library preset requires engines.review.enabled = false".to_owned());
    }
    let ci = &config.modules.ci;
    if !ci.enabled {
        return Err("library preset requires modules.ci.enabled = true".to_owned());
    }
    if ci.gate != "required-status-check" {
        return Err(
            "library preset requires modules.ci.gate = \"required-status-check\"".to_owned(),
        );
    }
    if ci.required_checks != [LIBRARY_REQUIRED_CHECK] {
        return Err("library preset requires modules.ci.required_checks = [\"CI\"]".to_owned());
    }
    if ci.commands.len() != LIBRARY_CI_COMMANDS.len()
        || ci
            .commands
            .iter()
            .zip(LIBRARY_CI_COMMANDS)
            .any(|(actual, expected)| actual != expected)
    {
        return Err(
            "library preset requires exactly four CI commands: fmt, clippy, test, doc".to_owned(),
        );
    }
    if ci.step_names.len() != LIBRARY_CI_COMMANDS.len()
        || ci.step_names.iter().any(|name| name.trim().is_empty())
    {
        return Err(
            "library preset requires one non-whitespace step name per CI command".to_owned(),
        );
    }
    let merge = &config.modules.merge;
    if !merge.enabled
        || merge.implementation != "human"
        || merge.gate != "required-ci-and-resolved-threads"
        || merge.auto_merge
    {
        return Err("library preset requires human merge with required CI and resolved threads; auto_merge must be false".to_owned());
    }
    Ok(())
}

// Slice A keeps library project checks internal. Public commands will use these checks in slice B.
#[allow(dead_code)]
pub fn validate_rust_library_project(root: &Path) -> Result<(), String> {
    for path in [".cargo/config", ".cargo/config.toml", "rust-toolchain"] {
        require_absent(root, path)?;
    }
    for path in ["src/main.rs", "src/bin", "examples", "benches", "build.rs"] {
        require_absent(root, path)?;
    }
    let manifest = required_toml(root, "Cargo.toml", "library preset requires Cargo.toml")?;
    if manifest.get("workspace").is_some() {
        return Err("library preset supports a single package and rejects [workspace]".to_owned());
    }
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "Cargo.toml needs a [package] table".to_owned())?;
    if package.contains_key("workspace") || package_uses_workspace_inheritance(package) {
        return Err(
            "library preset rejects package workspace inheritance and inherited package fields"
                .to_owned(),
        );
    }
    required_nonempty_string(package.get("name"), "package.name")?;
    required_nonempty_string(package.get("version"), "package.version")?;
    if package.get("edition").and_then(toml::Value::as_str) != Some("2024") {
        return Err("library preset requires package.edition = \"2024\"".to_owned());
    }
    require_enabled_boolean_flag(package, "[package]", "autotests")?;
    if package.get("autolib").and_then(toml::Value::as_bool) == Some(false)
        && manifest.get("lib").is_none()
    {
        return Err(
            "library preset does not support package.autolib = false without a [lib] target"
                .to_owned(),
        );
    }
    if manifest
        .get("lints")
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true)
    {
        return Err("library preset does not support [lints].workspace = true".to_owned());
    }
    validate_library_target(manifest.get("lib"))?;
    reject_binary_targets(&manifest)?;
    reject_local_path_dependencies(&manifest)?;
    reject_local_path_overrides(&manifest)?;
    validate_explicit_tests(root, manifest.get("test"))?;
    validate_discovered_integration_tests(root)?;

    let src = required_bytes(root, "src/lib.rs", "library preset requires src/lib.rs")?;
    ensure_nonempty_utf8_source(&src, "src/lib.rs")?;

    let toolchain = required_toml(
        root,
        "rust-toolchain.toml",
        "library preset requires rust-toolchain.toml",
    )?;
    let table = toolchain
        .get("toolchain")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "rust-toolchain.toml needs a [toolchain] table".to_owned())?;
    if table.get("channel").and_then(toml::Value::as_str) != Some("1.85") {
        return Err("rust-toolchain.toml must set toolchain.channel = \"1.85\"".to_owned());
    }
    let components = table
        .get("components")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| {
            "rust-toolchain.toml must set toolchain.components = [\"rustfmt\", \"clippy\"]"
                .to_owned()
        })?;
    let component_values: BTreeSet<_> = components
        .iter()
        .map(|value| {
            value.as_str().ok_or_else(|| {
                "rust-toolchain.toml toolchain.components must be strings".to_owned()
            })
        })
        .collect::<Result<_, _>>()?;
    if component_values != BTreeSet::from(["rustfmt", "clippy"]) {
        return Err(
            "rust-toolchain.toml toolchain.components must contain exactly rustfmt and clippy"
                .to_owned(),
        );
    }
    if table.contains_key("targets") {
        return Err(
            "rust-toolchain.toml toolchain.targets is unsupported for this preset".to_owned(),
        );
    }
    if table.contains_key("path") {
        return Err("rust-toolchain.toml toolchain.path is unsupported for this preset".to_owned());
    }

    let lock = required_toml(root, "Cargo.lock", "library preset requires Cargo.lock")?;
    if lock.as_table().is_none_or(toml::Table::is_empty) {
        return Err("Cargo.lock must be nonempty".to_owned());
    }
    if lock.get("version").and_then(toml::Value::as_integer) != Some(4) {
        return Err("Cargo.lock must set version = 4".to_owned());
    }
    Ok(())
}

fn package_uses_workspace_inheritance(package: &toml::Table) -> bool {
    package.values().any(|value| {
        value
            .as_table()
            .and_then(|table| table.get("workspace"))
            .and_then(toml::Value::as_bool)
            == Some(true)
    })
}

fn required_nonempty_string(value: Option<&toml::Value>, field: &str) -> Result<(), String> {
    if value
        .and_then(toml::Value::as_str)
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err(format!("Cargo.toml needs a nonempty {field}"));
    }
    Ok(())
}

fn validate_library_target(value: Option<&toml::Value>) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    let table = value
        .as_table()
        .ok_or_else(|| "Cargo.toml [lib] must be a table".to_owned())?;
    if let Some(path) = table.get("path") {
        let path = path
            .as_str()
            .ok_or_else(|| "Cargo.toml [lib].path must be a string".to_owned())?;
        if path != "src/lib.rs" {
            return Err(
                "library preset supports only the default library path src/lib.rs".to_owned(),
            );
        }
    }
    if let Some(crate_type) = table.get("crate-type") {
        let crate_type = crate_type
            .as_array()
            .ok_or_else(|| "Cargo.toml [lib].crate-type must be an array of strings".to_owned())?;
        if !matches!(
            crate_type.as_slice(),
            [toml::Value::String(value)] if value == "lib" || value == "rlib"
        ) {
            return Err(
                "library preset supports only crate-type = [\"lib\"] or [\"rlib\"]".to_owned(),
            );
        }
    }
    if table
        .get("proc-macro")
        .is_some_and(|value| value != &toml::Value::Boolean(false))
    {
        return Err("library preset does not support proc-macro libraries".to_owned());
    }
    for flag in ["test", "doctest", "harness", "doc"] {
        require_enabled_boolean_flag(table, "[lib]", flag)?;
    }
    Ok(())
}

fn reject_binary_targets(manifest: &toml::Value) -> Result<(), String> {
    for field in ["bin", "example", "bench"] {
        if manifest.get(field).is_some() {
            return Err(format!(
                "library preset does not support Cargo.toml [{field}] targets"
            ));
        }
    }
    match manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("build"))
    {
        Some(toml::Value::Boolean(false)) | None => Ok(()),
        Some(toml::Value::String(_)) => {
            Err("library preset does not support package.build scripts".to_owned())
        }
        Some(_) => Err("Cargo.toml package.build must be false or omitted".to_owned()),
    }
}

fn reject_local_path_dependencies(manifest: &toml::Value) -> Result<(), String> {
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        validate_dependency_table(manifest.get(section), section)?;
    }
    if let Some(targets) = manifest.get("target") {
        let targets = targets
            .as_table()
            .ok_or_else(|| "Cargo.toml [target] must be a table".to_owned())?;
        for (target, entry) in targets {
            let entry = entry
                .as_table()
                .ok_or_else(|| format!("Cargo.toml [target.{target}] must be a table"))?;
            for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                validate_dependency_table(
                    entry.get(section),
                    &format!("target.{target}.{section}"),
                )?;
            }
        }
    }
    Ok(())
}

fn reject_local_path_overrides(manifest: &toml::Value) -> Result<(), String> {
    for section in ["patch", "replace"] {
        let Some(table) = manifest.get(section) else {
            continue;
        };
        let table = table
            .as_table()
            .ok_or_else(|| format!("Cargo.toml [{section}] must be a table"))?;
        for (name, entry) in table {
            match entry {
                toml::Value::Table(registry_table) if section == "patch" => {
                    for (dependency, spec) in registry_table {
                        if spec
                            .as_table()
                            .is_some_and(|spec| spec.contains_key("path"))
                        {
                            return Err(format!(
                                "library preset does not support local path dependency {section}.{name}.{dependency}"
                            ));
                        }
                    }
                }
                toml::Value::Table(spec) if section == "replace" => {
                    if spec.contains_key("path") {
                        return Err(format!(
                            "library preset does not support local path dependency {section}.{name}"
                        ));
                    }
                }
                _ => return Err(format!("Cargo.toml [{section}.{name}] must be a table")),
            }
        }
    }
    Ok(())
}

fn validate_explicit_tests(root: &Path, tests: Option<&toml::Value>) -> Result<(), String> {
    let Some(tests) = tests else {
        return Ok(());
    };
    let tests = tests
        .as_array()
        .ok_or_else(|| "Cargo.toml [[test]] targets must be an array of tables".to_owned())?;
    for target in tests {
        let target = target
            .as_table()
            .ok_or_else(|| "Cargo.toml [[test]] targets must be tables".to_owned())?;
        let path = target
            .get("path")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| "Cargo.toml [[test]] targets need a string path".to_owned())?;
        if !is_top_level_tests_rs(path) {
            return Err("library preset supports only [[test]] paths under tests/*.rs".to_owned());
        }
        let bytes = required_bytes(
            root,
            path,
            "library preset [[test]] path is missing; keep tests in tests/*.rs",
        )?;
        ensure_nonempty_utf8_source(&bytes, path)?;
        for flag in ["test", "harness"] {
            require_enabled_boolean_flag(target, "[[test]]", flag)?;
        }
    }
    Ok(())
}

fn validate_discovered_integration_tests(root: &Path) -> Result<(), String> {
    let tests_dir = root.join("tests");
    match fs::symlink_metadata(&tests_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "library preset does not support symlink tests directory: {}",
                tests_dir.display()
            ));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(format!(
                "library preset requires tests to be a real directory: {}",
                tests_dir.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "library preset cannot inspect tests directory {}: {error}",
                tests_dir.display()
            ));
        }
    }
    let entries = fs::read_dir(&tests_dir).map_err(|error| {
        format!(
            "library preset cannot read tests directory {}: {error}",
            tests_dir.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "library preset cannot read tests directory {}: {error}",
                tests_dir.display()
            )
        })?;
        let relative = format!("tests/{}", entry.file_name().to_string_lossy());
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            format!(
                "library preset cannot inspect {}: {error}",
                entry.path().display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "library preset does not support symlink integration test entries: {relative}"
            ));
        }
        if Path::new(&relative)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
        {
            if !metadata.is_file() {
                return Err(format!(
                    "library preset requires regular integration test source files: {relative}"
                ));
            }
            let bytes = required_bytes(
                root,
                &relative,
                "library preset requires discovered tests/*.rs files to exist",
            )?;
            ensure_nonempty_utf8_source(&bytes, &relative)?;
            continue;
        }
        if metadata.is_dir() {
            let nested_main = format!("{relative}/main.rs");
            if read_regular(&entry.path().join("main.rs"))
                .map_err(|error| error.to_string())?
                .is_some()
            {
                return Err(format!(
                    "library preset supports only integration tests at tests/*.rs; found discovered target {nested_main}"
                ));
            }
        }
    }
    Ok(())
}

fn is_top_level_tests_rs(path: &str) -> bool {
    let path = Path::new(path);
    path.parent() == Some(Path::new("tests"))
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
        && path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn require_enabled_boolean_flag(
    table: &toml::Table,
    section: &str,
    field: &str,
) -> Result<(), String> {
    let Some(value) = table.get(field) else {
        return Ok(());
    };
    match value {
        toml::Value::Boolean(true) => Ok(()),
        toml::Value::Boolean(false) => Err(format!(
            "Cargo.toml {section}.{field} must be true or omitted"
        )),
        _ => Err(format!("Cargo.toml {section}.{field} must be a boolean")),
    }
}

fn validate_dependency_table(value: Option<&toml::Value>, section: &str) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    let dependencies = value
        .as_table()
        .ok_or_else(|| format!("Cargo.toml [{section}] must be a table"))?;
    for (name, spec) in dependencies {
        let Some(spec) = spec.as_table() else {
            continue;
        };
        if spec.contains_key("path") {
            return Err(format!(
                "library preset does not support local path dependency {section}.{name}"
            ));
        }
        if spec.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
            return Err(format!(
                "library preset does not support workspace-inherited dependency {section}.{name}"
            ));
        }
    }
    Ok(())
}

fn ensure_nonempty_utf8_source(bytes: &[u8], path: &str) -> Result<(), String> {
    std::str::from_utf8(bytes).map_err(|error| format!("{path} is not UTF-8: {error}"))?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(format!("{path} must be nonempty source"));
    }
    Ok(())
}

fn require_absent(root: &Path, path: &str) -> Result<(), String> {
    match read_regular(&root.join(path)).map_err(|error| error.to_string())? {
        Some(_) => Err(format!("library preset does not support {path}")),
        None => Ok(()),
    }
}

fn required_bytes(root: &Path, path: &str, missing: &str) -> Result<Vec<u8>, String> {
    read_regular(&root.join(path))
        .map_err(|error| error.to_string())?
        .ok_or_else(|| missing.to_owned())
}

fn required_toml(root: &Path, path: &str, missing: &str) -> Result<toml::Value, String> {
    let bytes = required_bytes(root, path, missing)?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(format!("{path} must be nonempty"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|error| format!("invalid {path}: {error}"))?;
    toml::from_str(text).map_err(|error| format!("invalid {path}: {error}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::semicolon_if_nothing_returned)]

    use super::{
        BuiltinPreset, LIBRARY_CI_COMMANDS, select_builtin_preset, validate_rust_library_config,
        validate_rust_library_project,
    };
    use crate::FactoryConfig;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn select_builtin_preset_accepts_only_cli_or_library_with_single_rust_language() {
        assert_eq!(
            select_builtin_preset("cli", &["rust".to_owned()]).unwrap(),
            BuiltinPreset::RustCli
        );
        assert_eq!(
            select_builtin_preset("library", &["rust".to_owned()]).unwrap(),
            BuiltinPreset::RustLibrary
        );
        for (profile, languages) in [
            ("web", vec!["rust".to_owned()]),
            ("library", vec!["typescript".to_owned()]),
            ("library", vec!["rust".to_owned(), "rust".to_owned()]),
            ("library", vec!["rust".to_owned(), "typescript".to_owned()]),
            ("library", vec![]),
        ] {
            assert!(
                select_builtin_preset(profile, &languages).is_err(),
                "unexpectedly accepted {profile:?} {languages:?}"
            );
        }
    }

    fn library_config() -> FactoryConfig {
        let mut config = FactoryConfig::from_toml(include_str!("../factory/factory.toml")).unwrap();
        config.profile = "library".to_owned();
        config.languages = vec!["rust".to_owned()];
        config.modules.ci.required_checks = vec!["CI".to_owned()];
        config.modules.ci.commands = LIBRARY_CI_COMMANDS
            .iter()
            .map(ToString::to_string)
            .collect();
        config.modules.ci.step_names = ["Fmt", "Clippy", "Tests", "Docs"]
            .map(ToString::to_string)
            .into();
        config.modules.review.enabled = false;
        config.engines.review.enabled = false;
        config
    }

    #[test]
    fn library_config_validation_requires_exact_ci_contract_and_disabled_review_engine() {
        let config = library_config();
        assert!(validate_rust_library_config(&config).is_ok());

        let mut changed = library_config();
        changed.engines.review.enabled = true;
        assert!(validate_rust_library_config(&changed).is_err());

        for (change, message) in [
            (
                Box::new(|config: &mut FactoryConfig| {
                    config.modules.ci.commands[0].push_str(" --workspace");
                }) as Box<dyn Fn(&mut FactoryConfig)>,
                "exactly four CI commands",
            ),
            (
                Box::new(|config: &mut FactoryConfig| {
                    config.modules.ci.commands.swap(0, 1);
                }),
                "exactly four CI commands",
            ),
            (
                Box::new(|config: &mut FactoryConfig| {
                    config.modules.ci.required_checks = vec!["CI".to_owned(), "Other".to_owned()];
                }),
                "required_checks",
            ),
            (
                Box::new(|config: &mut FactoryConfig| {
                    config.modules.ci.step_names[2] = "  ".to_owned();
                }),
                "step name",
            ),
        ] {
            let mut changed = library_config();
            change(&mut changed);
            assert!(
                validate_rust_library_config(&changed)
                    .unwrap_err()
                    .contains(message)
            );
        }
    }

    #[test]
    fn library_config_validation_rejects_non_library_modes() {
        let mut config = library_config();
        config.profile = "cli".to_owned();
        assert!(
            validate_rust_library_config(&config)
                .unwrap_err()
                .contains("unsupported library preset")
        );

        config = library_config();
        config.coordinator.automatic_dispatch = true;
        assert!(validate_rust_library_config(&config).is_err());

        config = library_config();
        config.modules.release.enabled = true;
        assert!(validate_rust_library_config(&config).is_err());
    }

    struct TempProject(PathBuf);

    impl TempProject {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "acme-preset-tests-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let project = Self(root);
            project.write(
                "Cargo.toml",
                "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            );
            project.write(
                "Cargo.lock",
                "version = 4\n[[package]]\nname = \"sample\"\nversion = \"0.1.0\"\n",
            );
            project.write(
                "src/lib.rs",
                "/// adds one\npub fn sample() -> u32 { 1 }\n\n#[cfg(test)]\nmod tests {\n    use super::sample;\n\n    #[test]\n    fn unit() {\n        assert_eq!(sample(), 1);\n    }\n}\n",
            );
            project.write(
                "tests/integration.rs",
                "use sample::sample;\n\n#[test]\nfn integration() { assert_eq!(sample(), 1); }\n",
            );
            project.write(
                "rust-toolchain.toml",
                "[toolchain]\nchannel = \"1.85\"\ncomponents = [\"rustfmt\", \"clippy\"]\n",
            );
            project
        }

        fn write(&self, path: &str, contents: &str) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }

        fn write_bytes(&self, path: &str, bytes: &[u8]) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }

        fn remove(&self, path: &str) {
            let path = self.0.join(path);
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                fs::remove_dir_all(path).unwrap();
            } else {
                fs::remove_file(path).unwrap();
            }
        }

        fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
            fn visit(
                base: &std::path::Path,
                path: &std::path::Path,
                out: &mut Vec<(String, Vec<u8>)>,
            ) {
                for entry in fs::read_dir(path).unwrap() {
                    let entry = entry.unwrap();
                    let path = entry.path();
                    let metadata = fs::symlink_metadata(&path).unwrap();
                    let relative = path
                        .strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    let file_type = metadata.file_type();
                    if file_type.is_symlink() {
                        out.push((
                            relative,
                            format!("symlink:{}", fs::read_link(&path).unwrap().display())
                                .into_bytes(),
                        ));
                        continue;
                    }
                    if metadata.is_dir() {
                        visit(base, &path, out);
                        out.push((relative, b"directory".to_vec()));
                        continue;
                    }
                    if metadata.is_file() {
                        out.push((relative, fs::read(&path).unwrap()));
                        continue;
                    }
                    out.push((relative, b"special".to_vec()));
                }
            }

            let mut out = Vec::new();
            visit(&self.0, &self.0, &mut out);
            out.sort_by(|a, b| a.0.cmp(&b.0));
            out
        }

        fn rejects_without_mutation(&self, expected: &str, mutate: impl FnOnce(&Self)) {
            mutate(self);
            let before = self.snapshot();
            let error = validate_rust_library_project(&self.0).unwrap_err();
            assert!(error.contains(expected), "{error}");
            assert_eq!(self.snapshot(), before);
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn library_project_validation_accepts_supported_library_variants() {
        let project = TempProject::new();
        assert!(validate_rust_library_project(&project.0).is_ok());

        for manifest in [
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\nautotests = true\n",
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\npath = \"src/lib.rs\"\n",
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\ndoc = true\n",
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\ncrate-type = [\"lib\"]\n",
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\ncrate-type = [\"rlib\"]\n",
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[test]]\nname = \"integration\"\npath = \"tests/integration.rs\"\n",
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\nautolib = false\n[lib]\npath = \"src/lib.rs\"\n",
        ] {
            let project = TempProject::new();
            project.write("Cargo.toml", manifest);
            assert!(
                validate_rust_library_project(&project.0).is_ok(),
                "{manifest}"
            );
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn library_project_validation_rejects_invalid_inputs_without_mutation() {
        for (mutate, expected) in [
            (
                Box::new(|project: &TempProject| project.remove("Cargo.toml"))
                    as Box<dyn Fn(&TempProject)>,
                "Cargo.toml",
            ),
            (
                Box::new(|project: &TempProject| project.write("Cargo.toml", "")),
                "must be nonempty",
            ),
            (
                Box::new(|project: &TempProject| project.write("Cargo.toml", "not = [")),
                "invalid Cargo.toml",
            ),
            (
                Box::new(|project: &TempProject| project.write_bytes("Cargo.toml", b"\xff\xfe")),
                "invalid Cargo.toml",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write(
                        "Cargo.toml",
                        "[package]\nname = 1\nversion = \"0.1.0\"\nedition = \"2024\"\n",
                    )
                }),
                "package.name",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write(
                        "Cargo.toml",
                        "[package]\nname = \"sample\"\nversion = 1\nedition = \"2024\"\n",
                    )
                }),
                "package.version",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write(
                        "Cargo.toml",
                        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = 2024\n",
                    )
                }),
                "edition",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[workspace]\nmembers=[]\n")
                }),
                "workspace",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname=\"sample\"\nversion={ workspace = true }\nedition=\"2024\"\n")
                }),
                "workspace inheritance",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2024\"\nautolib = false\n")
                }),
                "autolib",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2024\"\nautotests = false\n")
                }),
                "[package].autotests",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2024\"\nautotests = \"false\"\n")
                }),
                "[package].autotests",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[lib]\nproc-macro = true\n")
                }),
                "proc-macro",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[lib]\ncrate-type = [\"lib\", \"rlib\"]\n")
                }),
                "crate-type",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[lib]\ncrate-type = [\"lib\", \"lib\"]\n")
                }),
                "crate-type",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname=\"sample\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[lib]\npath=\"code/lib.rs\"\n")
                }),
                "default library path",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nlocal = { path = \"../local\" }\n")
                }),
                "local path dependency",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dev-dependencies]\nlocal = { path = \"../local\" }\n")
                }),
                "local path dependency",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[build-dependencies]\nlocal = { path = \"../local\" }\n")
                }),
                "local path dependency",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[target.'cfg(unix)'.dependencies]\nlocal = { path = \"../local\" }\n")
                }),
                "local path dependency",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[patch.crates-io]\nlocal = { path = \"../local\" }\n")
                }),
                "local path dependency",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[replace]\n\"local:0.1.0\" = { path = \"../local\" }\n")
                }),
                "local path dependency",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[bin]]\nname=\"other\"\npath=\"src/bin.rs\"\n")
                }),
                "bin",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[example]]\nname=\"ex\"\npath=\"examples/ex.rs\"\n")
                }),
                "example",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[bench]]\nname=\"bench\"\npath=\"benches/bench.rs\"\n")
                }),
                "bench",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n")
                }),
                "build scripts",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lints]\nworkspace = true\n")
                }),
                "lints",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[test]]\nname=\"integration\"\npath=\"other/integration.rs\"\n")
                }),
                "tests/*.rs",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("tests/nested/main.rs", "#[test]\nfn nested() {}\n");
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[test]]\nname=\"integration\"\npath=\"tests/nested/main.rs\"\n");
                }),
                "tests/*.rs",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.remove("tests/integration.rs");
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[test]]\nname=\"integration\"\npath=\"tests/integration.rs\"\n");
                }),
                "missing",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\ntest = false\n");
                }),
                "[lib].test",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\ntest = \"false\"\n");
                }),
                "[lib].test",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\ndoctest = false\n");
                }),
                "[lib].doctest",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\ndoctest = \"false\"\n");
                }),
                "[lib].doctest",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\ndoc = false\n");
                }),
                "[lib].doc",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\ndoc = \"false\"\n");
                }),
                "[lib].doc",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\nharness = false\n");
                }),
                "[lib].harness",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[lib]\nharness = \"false\"\n");
                }),
                "[lib].harness",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[test]]\nname = \"integration\"\npath = \"tests/integration.rs\"\ntest = false\n");
                }),
                "[[test]].test",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[test]]\nname = \"integration\"\npath = \"tests/integration.rs\"\ntest = \"false\"\n");
                }),
                "[[test]].test",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[test]]\nname = \"integration\"\npath = \"tests/integration.rs\"\nharness = false\n");
                }),
                "[[test]].harness",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("Cargo.toml", "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[[test]]\nname = \"integration\"\npath = \"tests/integration.rs\"\nharness = \"false\"\n");
                }),
                "[[test]].harness",
            ),
            (
                Box::new(|project: &TempProject| project.remove("rust-toolchain.toml")),
                "rust-toolchain.toml",
            ),
            (
                Box::new(|project: &TempProject| project.write("rust-toolchain.toml", "")),
                "must be nonempty",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("rust-toolchain.toml", "[toolchain")
                }),
                "invalid rust-toolchain.toml",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write_bytes("rust-toolchain.toml", b"\xff")
                }),
                "invalid rust-toolchain.toml",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("rust-toolchain.toml", "[toolchain]\nchannel = \"stable\"\ncomponents = [\"rustfmt\", \"clippy\"]\n")
                }),
                "channel",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write(
                        "rust-toolchain.toml",
                        "[toolchain]\nchannel = \"1.85\"\ncomponents = [\"rustfmt\"]\n",
                    )
                }),
                "components",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write(
                        "rust-toolchain.toml",
                        "[toolchain]\nchannel = \"1.85\"\ncomponents = [\"rustfmt\", 1]\n",
                    )
                }),
                "components must be strings",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("rust-toolchain.toml", "[toolchain]\nchannel = \"1.85\"\ncomponents = [\"rustfmt\", \"clippy\"]\ntargets = [\"wasm32-unknown-unknown\"]\n")
                }),
                "targets",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write("rust-toolchain.toml", "[toolchain]\nchannel = \"1.85\"\ncomponents = [\"rustfmt\", \"clippy\"]\npath = \"/tmp/rust\"\n")
                }),
                "toolchain.path",
            ),
            (
                Box::new(|project: &TempProject| project.write("rust-toolchain", "1.85\n")),
                "rust-toolchain",
            ),
            (
                Box::new(|project: &TempProject| project.remove("Cargo.lock")),
                "Cargo.lock",
            ),
            (
                Box::new(|project: &TempProject| project.write("Cargo.lock", "")),
                "must be nonempty",
            ),
            (
                Box::new(|project: &TempProject| project.write("Cargo.lock", "not = [")),
                "invalid Cargo.lock",
            ),
            (
                Box::new(|project: &TempProject| project.write_bytes("Cargo.lock", b"\xff")),
                "invalid Cargo.lock",
            ),
            (
                Box::new(|project: &TempProject| project.write("Cargo.lock", "version = 3\n")),
                "version = 4",
            ),
            (
                Box::new(|project: &TempProject| project.remove("src/lib.rs")),
                "src/lib.rs",
            ),
            (
                Box::new(|project: &TempProject| project.write("src/lib.rs", "")),
                "nonempty source",
            ),
            (
                Box::new(|project: &TempProject| project.write_bytes("src/lib.rs", b"\xff")),
                "not UTF-8",
            ),
            (
                Box::new(|project: &TempProject| project.write("src/main.rs", "fn main() {}\n")),
                "src/main.rs",
            ),
            (
                Box::new(|project: &TempProject| {
                    project.write(".cargo/config.toml", "[build]\ntarget-dir = \"target\"\n")
                }),
                ".cargo/config.toml",
            ),
        ] {
            let project = TempProject::new();
            project.rejects_without_mutation(expected, mutate);
        }
    }

    #[cfg(unix)]
    #[test]
    #[allow(clippy::too_many_lines)]
    #[allow(clippy::cognitive_complexity)]
    fn library_project_validation_rejects_symlinks_fifos_and_nonregular_paths() {
        use std::os::unix::fs::FileTypeExt;
        use std::os::unix::fs::symlink;
        use std::process::Command;

        let project = TempProject::new();
        project.remove("src/lib.rs");
        fs::create_dir(project.0.join("src/lib.rs")).unwrap();
        let before = project.snapshot();
        assert!(
            validate_rust_library_project(&project.0)
                .unwrap_err()
                .contains("not a regular file")
        );
        assert_eq!(project.snapshot(), before);

        let project = TempProject::new();
        fs::rename(
            project.0.join("Cargo.toml"),
            project.0.join("Cargo.real.toml"),
        )
        .unwrap();
        symlink(
            project.0.join("Cargo.real.toml"),
            project.0.join("Cargo.toml"),
        )
        .unwrap();
        let before = project.snapshot();
        assert!(
            validate_rust_library_project(&project.0)
                .unwrap_err()
                .contains("symlink")
        );
        assert_eq!(project.snapshot(), before);

        let project = TempProject::new();
        fs::create_dir(project.0.join(".cargo")).unwrap();
        project.write("config.real.toml", "[build]\ntarget-dir = \"target\"\n");
        symlink(
            project.0.join("config.real.toml"),
            project.0.join(".cargo/config.toml"),
        )
        .unwrap();
        let before = project.snapshot();
        assert!(
            validate_rust_library_project(&project.0)
                .unwrap_err()
                .contains("symlink")
        );
        assert_eq!(project.snapshot(), before);

        let project = TempProject::new();
        fs::remove_file(project.0.join("Cargo.lock")).unwrap();
        assert!(
            Command::new("mkfifo")
                .arg(project.0.join("Cargo.lock"))
                .status()
                .unwrap()
                .success()
        );
        assert!(
            fs::symlink_metadata(project.0.join("Cargo.lock"))
                .unwrap()
                .file_type()
                .is_fifo()
        );
        let before = project.snapshot();
        assert!(
            validate_rust_library_project(&project.0)
                .unwrap_err()
                .contains("regular file")
        );
        assert_eq!(project.snapshot(), before);

        let project = TempProject::new();
        fs::remove_file(project.0.join("tests/integration.rs")).unwrap();
        symlink(
            project.0.join("src/lib.rs"),
            project.0.join("tests/integration.rs"),
        )
        .unwrap();
        let before = project.snapshot();
        assert!(
            validate_rust_library_project(&project.0)
                .unwrap_err()
                .contains("symlink")
        );
        assert_eq!(project.snapshot(), before);

        let project = TempProject::new();
        fs::remove_file(project.0.join("tests/integration.rs")).unwrap();
        assert!(
            Command::new("mkfifo")
                .arg(project.0.join("tests/integration.rs"))
                .status()
                .unwrap()
                .success()
        );
        assert!(
            fs::symlink_metadata(project.0.join("tests/integration.rs"))
                .unwrap()
                .file_type()
                .is_fifo()
        );
        let before = project.snapshot();
        assert!(
            validate_rust_library_project(&project.0)
                .unwrap_err()
                .contains("regular integration test source files")
        );
        assert_eq!(project.snapshot(), before);

        let project = TempProject::new();
        fs::create_dir(project.0.join("tests/nested")).unwrap();
        project.write("tests/nested/main.rs", "#[test]\nfn nested() {}\n");
        let before = project.snapshot();
        assert!(
            validate_rust_library_project(&project.0)
                .unwrap_err()
                .contains("tests/*.rs")
        );
        assert_eq!(project.snapshot(), before);

        let project = TempProject::new();
        let parent = project.0.join("outside");
        fs::create_dir(&parent).unwrap();
        let link = project.0.join("src");
        fs::remove_dir_all(&link).unwrap();
        symlink(parent, &link).unwrap();
        let before = project.snapshot();
        assert!(
            validate_rust_library_project(&project.0)
                .unwrap_err()
                .contains("symlink directory component")
        );
        assert_eq!(project.snapshot(), before);

        let project = TempProject::new();
        let outside = project.0.join("outside-tests");
        fs::create_dir(&outside).unwrap();
        fs::remove_file(project.0.join("tests/integration.rs")).unwrap();
        fs::remove_dir(project.0.join("tests")).unwrap();
        symlink(&outside, project.0.join("tests")).unwrap();
        let before = project.snapshot();
        assert!(
            validate_rust_library_project(&project.0)
                .unwrap_err()
                .contains("symlink")
        );
        assert_eq!(project.snapshot(), before);

        let project = TempProject::new();
        fs::create_dir(project.0.join("outside-tests")).unwrap();
        fs::rename(
            project.0.join("tests/integration.rs"),
            project.0.join("outside-tests/main.rs"),
        )
        .unwrap();
        fs::remove_dir(project.0.join("tests")).unwrap();
        symlink(project.0.join("outside-tests"), project.0.join("tests")).unwrap();
        let before = project.snapshot();
        assert!(
            validate_rust_library_project(&project.0)
                .unwrap_err()
                .contains("symlink")
        );
        assert_eq!(project.snapshot(), before);

        let project = TempProject::new();
        fs::create_dir(project.0.join("tests-linked")).unwrap();
        project.write("tests-linked/main.rs", "#[test]\nfn nested() {}\n");
        fs::remove_file(project.0.join("tests/integration.rs")).unwrap();
        fs::remove_dir(project.0.join("tests")).unwrap();
        fs::create_dir(project.0.join("tests")).unwrap();
        symlink(
            project.0.join("tests-linked"),
            project.0.join("tests/nested"),
        )
        .unwrap();
        let before = project.snapshot();
        assert!(
            validate_rust_library_project(&project.0)
                .unwrap_err()
                .contains("symlink")
        );
        assert_eq!(project.snapshot(), before);
    }
}
