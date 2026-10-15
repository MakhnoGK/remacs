use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use remacs_config::project_name;
use remacs_core::Icon;
use serde::{Deserialize, Serialize};

/// Something to run, as a project's run file holds it: one `[[configurations]]` table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Configuration {
    pub name: String,
    #[serde(flatten)]
    pub kind: Kind,
    /// Passed on to the program.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Relative to the project root; left out, the kind's own directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Variables read from a dotenv file, relative to the working directory; `env` goes
    /// over them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_file: Option<String>,
    /// Shell commands run in turn first, in the working directory; one that fails stops
    /// the run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_launch: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum Kind {
    Cargo(Cargo),
    /// A script of a `package.json`, relative to the project root.
    Npm {
        package_json: String,
        script: String,
        /// Left out, the one whose lock file is nearest.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        package_manager: Option<PackageManager>,
    },
    /// A JavaScript or TypeScript file, relative to the project root, run by Node.
    Node {
        file: String,
    },
    /// A program, relative to the project root or looked up on `PATH`.
    Executable {
        program: String,
    },
    /// A command line, run by the shell `run.toml` [compile] names, as `compile` runs it.
    Shell {
        command: String,
    },
}

/// `cargo <command> -p <package> [--lib | --bin <bin> | --test <test> | --example
/// <example>] [<filter>]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Cargo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    /// The library's own tests.
    #[serde(default, skip_serializing_if = "is_false")]
    pub lib: bool,
    /// An integration test, a file of `tests/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,
    #[serde(default = "cargo_run", skip_serializing_if = "is_cargo_run")]
    pub command: String,
    /// The tests to run, by path: `cargo test` runs those whose path contains it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Given to cargo, before `--` and the program's arguments, e.g. `--release`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cargo_args: Vec<String>,
}

pub const CARGO_TEST: &str = "test";

impl Default for Cargo {
    fn default() -> Self {
        Self {
            package: None,
            bin: None,
            example: None,
            lib: false,
            test: None,
            command: cargo_run(),
            filter: None,
            cargo_args: Vec::new(),
        }
    }
}

impl Cargo {
    /// The package and target, as cargo's arguments: `-p api --bin api`.
    pub fn selection(&self) -> Vec<String> {
        let mut args = Vec::new();
        for (flag, value) in [
            ("-p", &self.package),
            ("--bin", &self.bin),
            ("--test", &self.test),
            ("--example", &self.example),
        ] {
            if let Some(value) = value {
                args.extend([flag.to_owned(), value.clone()]);
            }
        }
        if self.lib {
            args.push("--lib".to_owned());
        }
        args
    }

    pub fn runs_tests(&self) -> bool {
        self.command == CARGO_TEST
    }
}

fn cargo_run() -> String {
    "run".to_owned()
}

fn is_cargo_run(command: &str) -> bool {
    command == "run"
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl PackageManager {
    pub fn program(self) -> &'static str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Yarn => "yarn",
            PackageManager::Bun => "bun",
        }
    }
}

impl Kind {
    pub fn icon(&self) -> Icon {
        match self {
            Kind::Cargo(_) => Icon::Package,
            Kind::Npm { .. } => Icon::Script,
            Kind::Node { .. } => Icon::File,
            Kind::Executable { .. } | Kind::Shell { .. } => Icon::Terminal,
        }
    }

    /// The group the chooser lists it in.
    pub fn group(&self) -> &'static str {
        match self {
            Kind::Cargo(_) => "Cargo",
            Kind::Npm { .. } => "npm scripts",
            Kind::Node { .. } => "Files",
            Kind::Executable { .. } => "Executables",
            Kind::Shell { .. } => "Shell commands",
        }
    }

    /// Where its group comes in the chooser.
    pub fn group_rank(&self) -> u8 {
        match self {
            Kind::Cargo(_) => 0,
            Kind::Npm { .. } => 1,
            Kind::Node { .. } => 2,
            Kind::Executable { .. } => 3,
            Kind::Shell { .. } => 4,
        }
    }
}

impl Configuration {
    pub fn new(name: impl Into<String>, kind: Kind) -> Self {
        Self {
            name: name.into(),
            kind,
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            env_file: None,
            before_launch: Vec::new(),
        }
    }
}

/// What a run file holds.
#[derive(Default, Serialize, Deserialize)]
struct RunFile {
    #[serde(default)]
    configurations: Vec<Configuration>,
}

const HEADER: &str = "\
# Run configurations of
# {root}
#
# One [[configurations]] table each. remacs adds one whenever something is run for the
# first time; changes apply once the file is saved.
#
# kind = \"cargo\": package; bin, example, lib = true or test (a file of tests/);
#   command (\"run\"; \"test\" runs tests, those whose path contains filter); cargo-args
# kind = \"npm\": package-json, script, package-manager (npm, pnpm, yarn, bun)
# kind = \"node\": file (.js or .ts)
# kind = \"executable\": program
# kind = \"shell\": command, a command line run by the shell run.toml [compile] names
# Any kind: args, cwd, env, env-file, before-launch
";

/// A project's run configurations, read from its run file and read again once it
/// changes.
pub struct Project {
    pub configurations: Vec<Configuration>,
    /// The last one run without saving it, like IntelliJ's temporary configurations;
    /// kept until remacs quits.
    pub temporary: Option<Configuration>,
    /// The run file, if configurations are kept.
    path: Option<PathBuf>,
    read_at: Option<SystemTime>,
}

impl Project {
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn find(&self, name: &str) -> Option<&Configuration> {
        self.configurations
            .iter()
            .chain(&self.temporary)
            .find(|configuration| configuration.name == name)
    }

    /// The configuration that runs the same thing, e.g. the same binary.
    pub fn find_kind(&self, kind: &Kind) -> Option<&Configuration> {
        self.configurations
            .iter()
            .find(|configuration| configuration.kind == *kind)
    }

    /// `name`, or `name (2)` and so on when taken.
    pub fn unique_name(&self, name: &str) -> String {
        if self.find(name).is_none() {
            return name.to_owned();
        }
        (2..)
            .map(|number| format!("{name} ({number})"))
            .find(|candidate| self.find(candidate).is_none())
            .expect("unbounded")
    }

    /// Reads the file again if it changed since it was read; an error leaves the
    /// configurations as they were.
    fn reload(&mut self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let modified = fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok();
        if modified.is_some() && modified == self.read_at {
            return Ok(());
        }
        self.read_at = modified;
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.configurations.clear();
                return Ok(());
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let file: RunFile = toml::from_str(&text).map_err(|error| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            format!("{name}: {}", error.message())
        })?;
        self.configurations = file.configurations;
        Ok(())
    }

    /// Keeps `configuration` and appends it to the run file, which is created if needed;
    /// what else the file holds stays as it was written.
    fn add(&mut self, root: &Path, configuration: Configuration) -> io::Result<()> {
        if self.path.is_some() {
            let table = toml::to_string(&RunFile {
                configurations: vec![configuration.clone()],
            })
            .map_err(io::Error::other)?;
            self.append(root, &table)?;
        }
        self.configurations.push(configuration);
        Ok(())
    }

    /// Appends `text` to the run file, created with its header if need be.
    fn append(&mut self, root: &Path, text: &str) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Err(io::Error::other("Run configurations are not kept in files"));
        };
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }
        let header = if path.exists() {
            String::new()
        } else {
            HEADER.replace("{root}", &root.display().to_string())
        };
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        write!(file, "{header}\n{text}")?;
        self.read_at = fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok();
        Ok(())
    }
}

/// The run configurations of every project used so far, and which one each runs.
#[derive(Default)]
pub struct Store {
    /// Where run files are kept; none keeps configurations in memory only, e.g. in tests.
    dir: Option<PathBuf>,
    projects: HashMap<PathBuf, Project>,
    /// The configuration each project runs, by root; kept in `selection_file`.
    selected: BTreeMap<PathBuf, String>,
    selection_file: Option<PathBuf>,
}

impl Store {
    /// Keeps run files in `dir` and which configuration each project runs in `selection_file`.
    pub fn keep_in(&mut self, dir: PathBuf, selection_file: PathBuf) {
        self.selected = fs::read_to_string(&selection_file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        self.dir = Some(dir);
        self.selection_file = Some(selection_file);
        self.projects.clear();
    }

    /// The name of the configuration the project at `root` runs, if one was chosen.
    pub fn selected(&self, root: &Path) -> Option<&str> {
        self.selected.get(root).map(String::as_str)
    }

    pub fn select(&mut self, root: &Path, name: &str) -> io::Result<()> {
        if self.selected(root) == Some(name) {
            return Ok(());
        }
        self.selected.insert(root.to_path_buf(), name.to_owned());
        let Some(file) = &self.selection_file else {
            return Ok(());
        };
        if let Some(directory) = file.parent() {
            fs::create_dir_all(directory)?;
        }
        let json = serde_json::to_string_pretty(&self.selected).map_err(io::Error::other)?;
        fs::write(file, json)
    }

    /// The configurations of the project at `root`, read again if their file changed;
    /// with the error when it does not parse.
    pub fn project(&mut self, root: &Path) -> (&mut Project, Option<String>) {
        let path = self
            .dir
            .as_ref()
            .map(|dir| dir.join(format!("{}.toml", project_name(root))));
        let project = self
            .projects
            .entry(root.to_path_buf())
            .or_insert_with(|| Project {
                configurations: Vec::new(),
                temporary: None,
                path,
                read_at: None,
            });
        let error = project.reload().err();
        (project, error)
    }

    /// The configurations of `root` as last read, without reading them again.
    pub fn read(&self, root: &Path) -> Option<&Project> {
        self.projects.get(root)
    }

    /// The configuration of `root` that runs `configuration`'s kind, else
    /// `configuration` itself under a name not yet taken, saved.
    pub fn find_or_add(
        &mut self,
        root: &Path,
        configuration: Configuration,
    ) -> Result<Configuration, String> {
        let (project, _) = self.project(root);
        if let Some(existing) = project.find_kind(&configuration.kind) {
            return Ok(existing.clone());
        }
        // Saving the temporary one makes it permanent, under its own name.
        let temporary = project.temporary.take();
        if temporary
            .as_ref()
            .is_some_and(|temporary| temporary.kind != configuration.kind)
        {
            project.temporary = temporary;
        }
        let configuration = Configuration {
            name: project.unique_name(&configuration.name),
            ..configuration
        };
        project
            .add(root, configuration.clone())
            .map_err(|error| format!("Could not save the configuration: {error}"))?;
        Ok(configuration)
    }

    /// The configuration of `root` that runs `configuration`'s kind, else `configuration`
    /// itself as the project's temporary one, in place of the last.
    pub fn find_or_keep_temporary(
        &mut self,
        root: &Path,
        configuration: Configuration,
    ) -> Configuration {
        let (project, _) = self.project(root);
        if let Some(existing) = project.find_kind(&configuration.kind) {
            return existing.clone();
        }
        project.temporary = None;
        let configuration = Configuration {
            name: project.unique_name(&configuration.name),
            ..configuration
        };
        project.temporary = Some(configuration.clone());
        configuration
    }

    /// Appends a commented example to the run file of `root`, created if needed, and
    /// returns the file.
    pub fn add_template(&mut self, root: &Path) -> io::Result<PathBuf> {
        let (project, _) = self.project(root);
        project.append(root, TEMPLATE)?;
        Ok(project.path.clone().expect("appended to it"))
    }
}

/// What "New configuration…" adds: one to uncomment and change.
const TEMPLATE: &str = "\
# [[configurations]]
# name = \"all tests\"
# kind = \"cargo\"
# command = \"test\"
# cargo-args = [\"--workspace\"]
";
