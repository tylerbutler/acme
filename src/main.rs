use acme::{
    FactoryConfig, GitHubError, GitHubMethod, GitHubTransport, checkout_repository,
    run_doctor_fix_with_compiled_inputs, run_doctor_with_compiled_inputs,
};
use serde_json::Value;
use std::env;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        return Err("acme requires a command.".to_owned());
    };
    if command == "render" {
        return render(args);
    }
    if command == "compile" {
        return compile(args);
    }
    if command == "sync" {
        return sync(args);
    }
    if command == "init" {
        return init(args);
    }
    if command != "doctor" {
        return Err(format!("acme {command} is not implemented."));
    }

    let mut config_path = None;
    let mut compiled_inputs = None;
    let mut fix = false;
    while let Some(argument) = args.next() {
        if argument == "--fix" {
            fix = true;
            continue;
        }
        if argument != "--config" && argument != "--compiled-inputs" {
            return Err(format!("unknown doctor argument: {argument}"));
        }
        let path = args
            .next()
            .filter(|path| !path.starts_with("--") && !path.is_empty())
            .ok_or_else(|| format!("{argument} requires a path."))?;
        let target = if argument == "--config" {
            &mut config_path
        } else {
            &mut compiled_inputs
        };
        if target.replace(PathBuf::from(path)).is_some() {
            return Err(format!("{argument} must be specified once."));
        }
    }

    let config_path = config_path.unwrap_or_else(|| PathBuf::from("factory/factory.toml"));
    let config = FactoryConfig::load(&config_path).map_err(|error| error.to_string())?;
    let root =
        env::current_dir().map_err(|error| format!("failed to get current directory: {error}"))?;
    let report = if fix {
        let checkout = checkout_repository(&root)?;
        run_doctor_fix_with_compiled_inputs(
            &config,
            &root,
            &GhTransport,
            checkout.as_deref(),
            compiled_inputs.as_deref(),
        )
    } else {
        run_doctor_with_compiled_inputs(&config, &root, &GhTransport, compiled_inputs.as_deref())
    };
    println!("{report}");
    if report.is_success() {
        Ok(())
    } else {
        Err("doctor found drift or could not complete all checks.".to_owned())
    }
}

fn init(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut config_path = None;
    while let Some(argument) = args.next() {
        if argument != "--config" {
            return Err(format!("unknown init argument: {argument}"));
        }
        let path = args
            .next()
            .filter(|path| !path.starts_with("--") && !path.is_empty())
            .ok_or("--config requires a path.")?;
        if config_path.replace(PathBuf::from(path)).is_some() {
            return Err("--config must be specified once.".to_owned());
        }
    }
    let config_path = config_path.unwrap_or_else(|| PathBuf::from("factory/factory.toml"));
    let root =
        env::current_dir().map_err(|error| format!("failed to get current directory: {error}"))?;
    let report =
        acme::render::init_repository(&root, &config_path).map_err(|error| error.to_string())?;
    print!("{report}");
    let mut paths: Vec<_> = report
        .outputs
        .iter()
        .map(String::as_str)
        .chain(["factory/factory.toml", "factory/managed-files.json"])
        .collect();
    paths.sort_unstable();
    paths.dedup();
    let stage_paths = paths.join(" ");
    println!(
        "Init prepared local files only. Review before publishing:\n\
         git diff\n\
         git ls-files --others --exclude-standard\n\
         git switch -c factory-init\n\
         git add -- {stage_paths}\n\
         git diff --cached\n\
         git commit -m \"chore: initialize factory\"\n\
         gh pr create --draft\n\
         No provisioning or agent activation was performed."
    );
    Ok(())
}

fn sync(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut config_path = None;
    let mut compiled_inputs = None;
    let mut check = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--check" if !check => check = true,
            "--check" => return Err("--check must be specified once.".to_owned()),
            "--config" | "--compiled-inputs" => {
                let path = args
                    .next()
                    .filter(|path| !path.starts_with("--") && !path.is_empty())
                    .ok_or_else(|| format!("{argument} requires a path."))?;
                let target = if argument == "--config" {
                    &mut config_path
                } else {
                    &mut compiled_inputs
                };
                if target.replace(PathBuf::from(path)).is_some() {
                    return Err(format!("{argument} must be specified once."));
                }
            }
            _ => return Err(format!("unknown sync argument: {argument}")),
        }
    }
    let config_path = config_path.unwrap_or_else(|| PathBuf::from("factory/factory.toml"));
    let config = FactoryConfig::load(&config_path).map_err(|error| error.to_string())?;
    let root =
        env::current_dir().map_err(|error| format!("failed to get current directory: {error}"))?;
    let report = acme::render::sync_repository_with_compiled_inputs(
        &config,
        &root,
        check,
        compiled_inputs.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    print!("{report}");
    if check && !report.is_clean() {
        return Err("sync drift detected; run acme sync after review.".to_owned());
    }
    if report.is_clean() {
        println!("Sync is clean.");
    }
    Ok(())
}

fn compile(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut config_path = None;
    let mut destination = None;
    while let Some(argument) = args.next() {
        let target = match argument.as_str() {
            "--config" => &mut config_path,
            "--output" => &mut destination,
            _ => return Err(format!("unknown compile argument: {argument}")),
        };
        let path = args
            .next()
            .filter(|path| !path.starts_with("--") && !path.is_empty())
            .ok_or_else(|| format!("{argument} requires a path."))?;
        if target.replace(PathBuf::from(path)).is_some() {
            return Err(format!("{argument} must be specified once."));
        }
    }
    let destination = destination.ok_or("compile requires --output DIR.")?;
    let config_path = config_path.unwrap_or_else(|| PathBuf::from("factory/factory.toml"));
    let config = FactoryConfig::load(&config_path).map_err(|error| error.to_string())?;
    let repository =
        env::current_dir().map_err(|error| format!("failed to get current directory: {error}"))?;
    acme::render::compile_agent_workflows(&config, &repository, &destination)
        .map_err(|error| error.to_string())?;
    println!(
        "Compiled approved agent workflows to {}. No checkout synchronization or activation performed.",
        destination.display()
    );
    Ok(())
}

fn render(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let mut config_path = None;
    let mut destination = None;
    let mut compiled_inputs = None;
    while let Some(argument) = args.next() {
        let target = match argument.as_str() {
            "--config" => &mut config_path,
            "--output" => &mut destination,
            "--compiled-inputs" => &mut compiled_inputs,
            _ => return Err(format!("unknown render argument: {argument}")),
        };
        let path = args
            .next()
            .filter(|path| !path.starts_with("--") && !path.is_empty())
            .ok_or_else(|| format!("{argument} requires a path."))?;
        if target.replace(PathBuf::from(path)).is_some() {
            return Err(format!("{argument} must be specified once."));
        }
    }
    let destination = destination.ok_or("render requires --output DIR.")?;
    let config_path = config_path.unwrap_or_else(|| PathBuf::from("factory/factory.toml"));
    let config = FactoryConfig::load(&config_path).map_err(|error| error.to_string())?;
    let repository =
        env::current_dir().map_err(|error| format!("failed to get current directory: {error}"))?;
    acme::render::render_to_directory_with_compiled_inputs(
        &config,
        &repository,
        &destination,
        compiled_inputs.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    println!("Rendered approved config to {}.", destination.display());
    Ok(())
}

struct GhTransport;

impl GitHubTransport for GhTransport {
    fn write(&self, method: GitHubMethod, endpoint: &str, body: &Value) -> Result<(), GitHubError> {
        let mut child = Command::new("gh")
            .args([
                "api",
                "--hostname",
                "github.com",
                "--method",
                method.as_str(),
                "--input",
                "-",
                endpoint,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| GitHubError::ApiFailure)?;
        let input = child.stdin.take().ok_or(GitHubError::ApiFailure);
        let sent = input.and_then(|mut input| {
            input
                .write_all(body.to_string().as_bytes())
                .map_err(|_| GitHubError::ApiFailure)
        });
        let output = child
            .wait_with_output()
            .map_err(|_| GitHubError::ApiFailure)?;
        sent?;
        if output.status.success() {
            Ok(())
        } else {
            Err(github_error(&output.stderr))
        }
    }

    fn get(&self, endpoint: &str) -> Result<Vec<Value>, GitHubError> {
        let output = Command::new("gh")
            .args([
                "api",
                "--paginate",
                "--slurp",
                "--hostname",
                "github.com",
                endpoint,
            ])
            .output()
            .map_err(|_| GitHubError::ApiFailure)?;
        if !output.status.success() {
            return Err(github_error(&output.stderr));
        }

        let pages: Vec<Value> =
            serde_json::from_slice(&output.stdout).map_err(|_| GitHubError::ApiFailure)?;
        Ok(pages)
    }
}

fn github_error(stderr: &[u8]) -> GitHubError {
    let error = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if error.contains("http 401") || error.contains("not logged in") {
        GitHubError::AuthenticationRequired
    } else if error.contains("http 403") || error.contains("resource not accessible") {
        GitHubError::PermissionDenied
    } else {
        GitHubError::ApiFailure
    }
}
