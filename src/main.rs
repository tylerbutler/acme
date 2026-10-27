use acme::{
    FactoryConfig, GitHubError, GitHubMethod, GitHubTransport, checkout_repository, run_doctor,
    run_doctor_fix,
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
    if command != "doctor" {
        return Err(format!("acme {command} is not implemented."));
    }

    let mut config_path = PathBuf::from("factory/factory.toml");
    let mut fix = false;
    while let Some(argument) = args.next() {
        if argument == "--fix" {
            fix = true;
            continue;
        }
        if argument != "--config" {
            return Err(format!("unknown doctor argument: {argument}"));
        }
        let Some(path) = args.next() else {
            return Err("--config requires a path.".to_owned());
        };
        config_path = PathBuf::from(path);
    }

    let config = FactoryConfig::load(&config_path).map_err(|error| error.to_string())?;
    let root =
        env::current_dir().map_err(|error| format!("failed to get current directory: {error}"))?;
    let report = if fix {
        let checkout = checkout_repository(&root)?;
        run_doctor_fix(&config, &root, &GhTransport, checkout.as_deref())
    } else {
        run_doctor(&config, &root, &GhTransport)
    };
    println!("{report}");
    if report.is_success() {
        Ok(())
    } else {
        Err("doctor found drift or could not complete all checks.".to_owned())
    }
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
