//! Installers: package managers (npm, go, pip…) described as data, in the built-in
//! `installers.toml` and `tools.toml` `[installers]`, so that one more needs no code.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;

use serde::Deserialize;
use serde_json::Value;

use crate::catalog::fill;
use crate::find_on_path;
use crate::install::run;

const BUILT_IN: &str = include_str!("../installers.toml");
/// The version of a tool whose installer cannot look up versions.
pub const UNVERSIONED: &str = "latest";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Installer {
    /// Programs that must be on `PATH` for it to run.
    pub needs: Vec<String>,
    pub latest: Option<Latest>,
    /// Commands run in order.
    pub install: Vec<Vec<String>>,
    /// The commands' environment.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Where the program is, in the version's directory.
    pub program: String,
    /// The program's environment.
    #[serde(default)]
    pub run_env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Latest {
    /// A command printing the version.
    Command { command: Vec<String> },
    /// A URL answering JSON, and the JSON pointer to the version in it.
    Url { url: String, version: String },
}

/// What fills an installer's templates.
pub struct Vars<'a> {
    pub dir: &'a Path,
    pub package: &'a str,
    pub version: &'a str,
    pub program: &'a str,
}

impl Vars<'_> {
    pub fn fill(&self, template: &str) -> String {
        fill(template)
            .replace("{dir}", &self.dir.display().to_string())
            .replace("{package}", self.package)
            .replace("{version}", self.version)
            .replace("{program}", self.program)
    }
}

pub fn built_in() -> BTreeMap<String, Installer> {
    toml::from_str(BUILT_IN).expect("the built-in installers are valid")
}

impl Installer {
    /// Whether what it needs is on `PATH`.
    pub fn can_run(&self) -> bool {
        self.needs.iter().all(|program| find_on_path(program).is_some())
    }

    pub fn latest(&self, vars: &Vars) -> Result<String, String> {
        match &self.latest {
            None => Ok(UNVERSIONED.to_owned()),
            Some(Latest::Command { command }) => {
                let (program, args) = command.split_first().ok_or("no command")?;
                let output = Command::new(vars.fill(program))
                    .args(args.iter().map(|arg| vars.fill(arg)))
                    .stdin(Stdio::null())
                    .output()
                    .map_err(|error| format!("could not run {program}: {error}"))?;
                let found = String::from_utf8_lossy(&output.stdout);
                match found.lines().map(str::trim).rfind(|line| !line.is_empty()) {
                    Some(version) if output.status.success() => Ok(version.to_owned()),
                    _ => Err(error_line(&output.stderr)),
                }
            }
            Some(Latest::Url { url, version }) => {
                let output = Command::new("curl")
                    .args(["-fsSL", "-A", "remacs"])
                    .arg(vars.fill(url))
                    .stdin(Stdio::null())
                    .output()
                    .map_err(|error| format!("could not run curl: {error}"))?;
                if !output.status.success() {
                    return Err(error_line(&output.stderr));
                }
                let answer: Value = serde_json::from_slice(&output.stdout)
                    .map_err(|error| format!("unexpected answer from {url}: {error}"))?;
                answer
                    .pointer(version)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| format!("no version of {} at {url}", vars.package))
            }
        }
    }

    /// Runs the install commands, stopping at the first that fails or once `cancelled`;
    /// an argument `{with}` stands for the packages of `with`, none or several.
    pub fn install(
        &self,
        vars: &Vars,
        with: &[&str],
        cancelled: &AtomicBool,
    ) -> Result<(), String> {
        let env: Vec<_> = self.env.iter().map(|(name, value)| (name, vars.fill(value))).collect();
        for command in &self.install {
            let (program, args) = command.split_first().ok_or("an empty install command")?;
            let program = vars.fill(program);
            let args = args.iter().flat_map(|arg| match arg.as_str() {
                "{with}" => with.iter().map(|&package| package.to_owned()).collect(),
                _ => vec![vars.fill(arg)],
            });
            let child = Command::new(&program)
                .args(args)
                .envs(env.iter().map(|(name, value)| (name, value)))
                .current_dir(vars.dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| format!("could not run {program}: {error}"))?;
            let output = run(child, cancelled, || {})?;
            if !output.status.success() {
                let name = Path::new(&program)
                    .file_name()
                    .map_or(program.clone(), |name| name.to_string_lossy().into());
                return Err(format!("{name} failed: {}", error_line(&output.stderr)));
            }
        }
        Ok(())
    }

    pub fn run_env(&self, vars: &Vars) -> Vec<(String, String)> {
        self.run_env.iter().map(|(name, value)| (name.clone(), vars.fill(value))).collect()
    }
}

/// The line of a failed command's errors that says most.
pub fn error_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<_> = text
        .lines()
        .map(|line| line.trim().trim_start_matches("npm error").trim())
        .filter(|line| !line.is_empty() && !line.starts_with("code "))
        .collect();
    lines
        .iter()
        .find(|line| line.to_lowercase().contains("error"))
        .or(lines.first())
        .map_or_else(|| "no reason given".to_owned(), |line| (*line).to_owned())
}
