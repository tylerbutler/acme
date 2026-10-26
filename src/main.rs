use acme::{FactoryConfig, GitHubError, GitHubTransport, run_doctor};
use serde_json::Value;
use std::env;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

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
    while let Some(argument) = args.next() {
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
    let report = run_doctor(&config, &root, &GhTransport);
    println!("{report}");
    if report.is_success() {
        Ok(())
    } else {
        Err("doctor found drift or could not complete all checks.".to_owned())
    }
}

struct GhTransport;

impl GitHubTransport for GhTransport {
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
            let error = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
            return Err(
                if error.contains("http 401") || error.contains("not logged in") {
                    GitHubError::AuthenticationRequired
                } else if error.contains("http 403") || error.contains("resource not accessible") {
                    GitHubError::PermissionDenied
                } else {
                    GitHubError::ApiFailure
                },
            );
        }

        let pages: Vec<Value> =
            serde_json::from_slice(&output.stdout).map_err(|_| GitHubError::ApiFailure)?;
        Ok(pages)
    }
}
