use acme::FactoryConfig;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// Keep test preparation outside the checkout observed by render/sync assertions.
pub struct ReviewFixture(PathBuf);

impl ReviewFixture {
    #[must_use]
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "acme-reviewed-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    /// Only fixture setup runs fake tools. The tested commands have no tools.
    #[must_use]
    pub fn refresh(&self, repository: &Path, config_path: &Path) -> Option<PathBuf> {
        let config = FactoryConfig::load(repository.join(config_path)).ok()?;
        if !config.modules.intake.enabled && !config.modules.build_cloud.enabled {
            return None;
        }
        let destination = self.0.join("review");
        if acme::render::build_agent_source_plan(&config, repository).is_err() {
            return Some(destination);
        }
        if destination.exists() {
            fs::remove_dir_all(&destination).unwrap();
        }
        let output = compile_fixture(
            repository,
            config_path,
            &destination,
            &self.0.join("bin"),
            "",
        );
        assert!(
            output.status.success(),
            "review fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Some(destination)
    }
}

impl Default for ReviewFixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ReviewFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[cfg(unix)]
fn executable(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(unix)]
#[must_use]
pub fn compile_fixture(
    repository: &Path,
    config_path: &Path,
    destination: &Path,
    bin: &Path,
    behavior: &str,
) -> Output {
    fs::create_dir_all(bin).unwrap();
    let config = FactoryConfig::load(repository.join(config_path)).unwrap();
    let mut arguments =
        "compile --no-check-update --strict --action-mode action --action-tag v0.93.5".to_owned();
    if config.modules.build_cloud.enabled {
        arguments.push_str(" .github/workflows/build-cloud.md");
    }
    if config.modules.intake.enabled {
        arguments.push_str(" .github/workflows/intake.md");
    }
    let count = arguments.split_whitespace().count();
    executable(
        &bin.join("gh-aw"),
        &format!(
            "#!/bin/sh\n\
             case \"$1\" in\n\
             version) printf 'gh aw version v0.93.5\\n'; exit 0;;\n\
             compile)\n\
             test \"$#\" -eq {count} || exit 85\n\
             test \"$*\" = '{arguments}' || exit 85\n\
             test -z \"$GH_TOKEN$GITHUB_TOKEN$GH_AW_MODEL$GH_AW_ACTION_TAG\" || exit 80\n\
             test -z \"$GH_AW_DEFAULT_DETECTION_MODEL$GHAW_POLICY_MODELS_ALLOWED$GHAW_POLICY_MODELS_BLOCKED\" || exit 84\n\
             test \"$HOME\" != '{root}' || exit 81\n\
             test \"$HTTPS_PROXY\" = 'http://127.0.0.1:9' || exit 82\n\
             test -z \"$NO_PROXY\" || exit 83\n\
             {behavior}\n\
             for source in .github/workflows/*.md; do\n\
               lock=\"${{source%.md}}.lock.yml\"\n\
               /bin/cat '{fixtures}'/\"$lock\" > \"$lock\" || exit 86\n\
             done\n\
             exit 0;;\n\
             esac\n\
             exit 90\n",
            root = repository.display(),
            fixtures = env!("CARGO_MANIFEST_DIR"),
        ),
    );
    executable(
        &bin.join("actionlint"),
        "#!/bin/sh\ncase \"$1\" in -version) printf 'v1.7.12\\n';; esac\nexit 0\n",
    );
    Command::new(env!("CARGO_BIN_EXE_acme"))
        .current_dir(repository)
        .env("PATH", bin)
        .env("HOME", repository)
        .env("GH_TOKEN", "test-token-must-not-reach-child")
        .env("GH_AW_MODEL", "ambient-override")
        .env("GH_AW_ACTION_TAG", "ambient-tag")
        .env("GH_AW_DEFAULT_DETECTION_MODEL", "ambient-detection-model")
        .env("GHAW_POLICY_MODELS_ALLOWED", "ambient-only-model")
        .env("GHAW_POLICY_MODELS_BLOCKED", "gpt-4.1")
        .args(["compile", "--config"])
        .arg(config_path)
        .arg("--output")
        .arg(destination)
        .output()
        .unwrap()
}

#[cfg(not(unix))]
#[must_use]
pub fn compile_fixture(_: &Path, _: &Path, _: &Path, _: &Path, _: &str) -> Output {
    panic!("the shell-backed compiled review fixture requires Unix")
}
