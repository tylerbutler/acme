use serde_json::Value;
use std::process::{Command, Stdio};

/// Implement reads with authenticated server responses. Do not substitute cached checkout data.
pub trait GitHubClient {
    fn get(&self, endpoint: &str) -> Result<Value, String>;
    fn file(&self, repository: &str, path: &str, revision: &str) -> Result<String, String>;
    fn graphql(&self, query: &str) -> Result<Value, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GhClient;

impl GhClient {
    fn read(arguments: &[&str]) -> Result<String, String> {
        let output = Command::new("gh")
            .args([
                "api",
                "--hostname",
                "github.com",
                "-H",
                "X-GitHub-Api-Version: 2026-03-10",
            ])
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .map_err(|_| "cannot start authenticated GitHub reader".to_owned())?;
        if !output.status.success() {
            return Err(
                "authenticated GitHub read failed; check access and API support".to_owned(),
            );
        }
        String::from_utf8(output.stdout).map_err(|_| "GitHub returned non-UTF-8 data".to_owned())
    }
}

impl GitHubClient for GhClient {
    fn get(&self, endpoint: &str) -> Result<Value, String> {
        if !endpoint.starts_with("repos/")
            && !endpoint.starts_with("agents/repos/")
            && endpoint != "user"
        {
            return Err("unsupported GitHub read endpoint".to_owned());
        }
        let text = Self::read(&["--method", "GET", endpoint])?;
        serde_json::from_str(&text).map_err(|_| "GitHub returned invalid JSON".to_owned())
    }

    fn file(&self, repository: &str, path: &str, revision: &str) -> Result<String, String> {
        let endpoint = format!(
            "repos/{repository}/contents/{}?ref={}",
            encode(path),
            encode(revision)
        );
        Self::read(&[
            "--method",
            "GET",
            "-H",
            "Accept: application/vnd.github.raw+json",
            &endpoint,
        ])
    }

    fn graphql(&self, query: &str) -> Result<Value, String> {
        if !query.starts_with("query ") {
            return Err("only GraphQL queries are supported".to_owned());
        }
        let text = Self::read(&["graphql", "-f", &format!("query={query}")])?;
        let value: Value =
            serde_json::from_str(&text).map_err(|_| "GitHub returned invalid JSON".to_owned())?;
        if value.get("errors").is_some() {
            return Err("GitHub GraphQL evidence is incomplete".to_owned());
        }
        Ok(value)
    }
}

pub(super) fn encode(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~/".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("writing to a string cannot fail");
        }
    }
    encoded
}
