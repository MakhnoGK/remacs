//! What a configuration runs: the program, its arguments, directory and environment.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use remacs_files::expand_home;

use crate::configuration::{Configuration, Kind, PackageManager};
use crate::settings::{RunSettings, TypeScriptRuntime};

/// A program to start, with the shell commands to run before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Over the environment remacs was started in.
    pub env: Vec<(String, String)>,
    /// Shell commands, run in turn in `cwd` first.
    pub before: Vec<String>,
}

impl Launch {
    /// As a shell would take it, e.g. `cargo run -p api`.
    pub fn command_line(&self) -> String {
        let program = Path::new(&self.program)
            .file_name()
            .map_or(self.program.as_str(), |name| {
                name.to_str().unwrap_or_default()
            });
        std::iter::once(program)
            .chain(self.args.iter().map(String::as_str))
            .map(quote)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// `word` as a shell reads it back, quoted only when it has to be.
fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_./:=@%+,".contains(character));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// What runs `configuration` of the project at `root`.
pub fn launch(
    configuration: &Configuration,
    root: &Path,
    settings: &RunSettings,
) -> Result<Launch, String> {
    let (program, mut args, own_directory) = match &configuration.kind {
        Kind::Cargo(cargo) => {
            let mut args = vec![cargo.command.clone()];
            args.extend(cargo.selection());
            args.extend(cargo.cargo_args.iter().cloned());
            args.extend(cargo.filter.iter().cloned());
            if !configuration.args.is_empty() {
                args.push("--".to_owned());
            }
            ("cargo".to_owned(), args, root.to_path_buf())
        }
        Kind::Npm {
            package_json,
            script,
            package_manager,
        } => {
            let directory = root
                .join(package_json)
                .parent()
                .map_or_else(|| root.to_path_buf(), Path::to_path_buf);
            let manager =
                package_manager.unwrap_or_else(|| detect_package_manager(&directory, root));
            let mut args = vec!["run".to_owned(), script.clone()];
            // npm takes the script's own arguments only after `--`.
            if manager == PackageManager::Npm && !configuration.args.is_empty() {
                args.push("--".to_owned());
            }
            (manager.program().to_owned(), args, directory)
        }
        Kind::Node { file } => {
            let (program, mut args) = if is_typescript(file) {
                typescript_command(settings.typescript, node_version(), || {
                    find_tsx(&root.join(file), root)
                })?
            } else {
                ("node".to_owned(), Vec::new())
            };
            args.push(file.clone());
            (program, args, root.to_path_buf())
        }
        Kind::Executable { program } => {
            let program = if program.contains('/') {
                root.join(expand_home(program)).display().to_string()
            } else {
                program.clone()
            };
            (program, Vec::new(), root.to_path_buf())
        }
    };
    args.extend(configuration.args.iter().cloned());
    let cwd = configuration
        .cwd
        .as_ref()
        .map_or(own_directory, |cwd| root.join(expand_home(cwd)));
    let mut env: Vec<(String, String)> = settings
        .env
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    if let Some(env_file) = &configuration.env_file {
        let path = cwd.join(expand_home(env_file));
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        env.extend(parse_dotenv(&text));
    }
    env.extend(
        configuration
            .env
            .iter()
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    Ok(Launch {
        program,
        args,
        cwd,
        env,
        before: configuration.before_launch.clone(),
    })
}

pub fn is_typescript(file: &str) -> bool {
    [".ts", ".mts", ".cts"]
        .iter()
        .any(|extension| file.ends_with(extension))
}

/// The package manager whose lock file is nearest to `directory`, up to `root`; npm when
/// there is none.
pub fn detect_package_manager(directory: &Path, root: &Path) -> PackageManager {
    const LOCK_FILES: [(&str, PackageManager); 5] = [
        ("pnpm-lock.yaml", PackageManager::Pnpm),
        ("bun.lock", PackageManager::Bun),
        ("bun.lockb", PackageManager::Bun),
        ("yarn.lock", PackageManager::Yarn),
        ("package-lock.json", PackageManager::Npm),
    ];
    directory
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(root))
        .find_map(|ancestor| {
            LOCK_FILES
                .iter()
                .find(|(lock, _)| ancestor.join(lock).is_file())
                .map(|&(_, manager)| manager)
        })
        .unwrap_or(PackageManager::Npm)
}

/// Major, minor and patch.
pub type Version = (u32, u32, u32);

/// The program and the arguments before the file that run TypeScript with `runtime` and
/// Node at `node`; `tsx` is looked for only when needed.
pub fn typescript_command(
    runtime: TypeScriptRuntime,
    node: Option<Version>,
    tsx: impl FnOnce() -> Option<PathBuf>,
) -> Result<(String, Vec<String>), String> {
    let node_command = |node: Option<Version>| {
        let flag = node.is_some_and(|version| !strips_types_by_default(version));
        let args = flag.then(|| "--experimental-strip-types".to_owned());
        ("node".to_owned(), args.into_iter().collect())
    };
    let can_strip = node.is_some_and(|version| version >= (22, 6, 0));
    match runtime {
        TypeScriptRuntime::Node => Ok(node_command(node)),
        TypeScriptRuntime::Auto if can_strip => Ok(node_command(node)),
        TypeScriptRuntime::Auto | TypeScriptRuntime::Tsx => tsx()
            .map(|tsx| (tsx.display().to_string(), Vec::new()))
            .ok_or_else(|| {
                "No tsx in node_modules: install it, or run on Node 22.6 or later".to_owned()
            }),
    }
}

/// Node strips types without being asked from 22.18 and 23.6 on.
fn strips_types_by_default(version: Version) -> bool {
    version >= (23, 6, 0) || (version >= (22, 18, 0) && version.0 == 22)
}

/// `node_modules/.bin/tsx` nearest to `file`, up to `root`.
fn find_tsx(file: &Path, root: &Path) -> Option<PathBuf> {
    file.ancestors()
        .skip(1)
        .take_while(|ancestor| ancestor.starts_with(root))
        .map(|ancestor| ancestor.join("node_modules/.bin/tsx"))
        .find(|tsx| tsx.is_file())
}

/// The version of the `node` on `PATH`, asked once.
fn node_version() -> Option<Version> {
    static VERSION: OnceLock<Option<Version>> = OnceLock::new();
    *VERSION.get_or_init(|| {
        let output = Command::new("node").arg("--version").output().ok()?;
        parse_version(&String::from_utf8_lossy(&output.stdout))
    })
}

/// `(22, 6, 0)` from `v22.6.0`.
pub fn parse_version(text: &str) -> Option<Version> {
    let mut numbers = text
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|number| number.parse().ok());
    Some((numbers.next()??, numbers.next()??, numbers.next()??))
}

/// Variables of a dotenv file: `NAME=value` lines, `export` and quotes allowed, `#`
/// comments.
pub fn parse_dotenv(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (name, value) = line.split_once('=')?;
            let value = value.trim();
            let value = match value.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let inner = value[1..].split(quote).next().unwrap_or_default();
                    if quote == '"' {
                        inner.replace("\\n", "\n")
                    } else {
                        inner.to_owned()
                    }
                }
                _ => value
                    .split(" #")
                    .next()
                    .unwrap_or_default()
                    .trim_end()
                    .to_owned(),
            };
            Some((name.trim().to_owned(), value))
        })
        .collect()
}
