//! Where programs start: a Rust binary's or example's `main`, a JavaScript or TypeScript
//! file, a `package.json` script, a Makefile's target, a justfile's recipe. Each comes
//! with the configuration that runs it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::configuration::{Cargo, Configuration, Kind};
use crate::rust_tests::rust_tests;
use crate::tasks::{is_task_file, task_entry_points};

pub const RUST_MODE: &str = "Rust";
pub const JAVASCRIPT_MODES: &[&str] = &["JavaScript", "TypeScript"];
pub const PACKAGE_JSON: &str = "package.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryPoint {
    pub line: usize,
    /// The last line of what it runs, e.g. a test function's, which runs from inside it.
    pub end_line: usize,
    /// Named for a new configuration; the project may already have one of its kind.
    pub configuration: Configuration,
    /// Run without saving a configuration, as one-off runs of a test are.
    pub temporary: bool,
}

impl EntryPoint {
    pub(crate) fn at(line: usize, configuration: Configuration) -> Self {
        Self { line, end_line: line, configuration, temporary: false }
    }
}

/// Whether a file in `mode` named `name` can have entry points at all.
pub fn may_have_entry_points(mode: &str, name: &str) -> bool {
    mode == RUST_MODE
        || JAVASCRIPT_MODES.contains(&mode)
        || name == PACKAGE_JSON
        || is_task_file(name)
}

/// The entry points of `text`, the file at `path` in `mode`, in the project at `root`.
pub fn entry_points(path: &Path, mode: &str, text: &str, root: &Path) -> Vec<EntryPoint> {
    entry_points_with(path, mode, text, root, &Manifests::default())
}

/// As `entry_points`, with the Cargo manifests read for other files of the same pass.
pub fn entry_points_with(
    path: &Path,
    mode: &str,
    text: &str,
    root: &Path,
    manifests: &Manifests,
) -> Vec<EntryPoint> {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    if mode == RUST_MODE {
        let Some(package) = CargoPackage::of(path, root, manifests) else {
            return Vec::new();
        };
        let mut points: Vec<EntryPoint> = rust_main(path, text, &package).into_iter().collect();
        points.extend(rust_tests(path, text, &package));
        points
    } else if JAVASCRIPT_MODES.contains(&mode) {
        script_file(path, name, root).into_iter().collect()
    } else if name == PACKAGE_JSON {
        package_scripts(path, text, root)
    } else {
        task_entry_points(path, text, root).unwrap_or_default()
    }
}

pub(crate) fn relative(path: &Path, root: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative.as_os_str().is_empty() { ".".to_owned() } else { relative.display().to_string() }
}

/// Cargo manifests read so far, by directory, so that a pass over the files of one
/// workspace reads and parses each once.
#[derive(Default)]
pub struct Manifests(RefCell<HashMap<PathBuf, Option<Rc<toml::Table>>>>);

impl Manifests {
    /// The `Cargo.toml` of `directory`, if it has one that parses.
    fn get(&self, directory: &Path) -> Option<Rc<toml::Table>> {
        self.0
            .borrow_mut()
            .entry(directory.to_path_buf())
            .or_insert_with(|| {
                let text = fs::read_to_string(directory.join("Cargo.toml")).ok()?;
                text.parse().ok().map(Rc::new)
            })
            .clone()
    }
}

/// The Cargo package a file belongs to.
pub(crate) struct CargoPackage<'a> {
    pub name: String,
    pub dir: PathBuf,
    pub manifest: Rc<toml::Table>,
    root: &'a Path,
    manifests: &'a Manifests,
}

impl<'a> CargoPackage<'a> {
    /// The package whose `Cargo.toml` is nearest above `path`, up to `root`.
    fn of(path: &Path, root: &'a Path, manifests: &'a Manifests) -> Option<Self> {
        let (dir, manifest) =
            path.ancestors().skip(1).take_while(|ancestor| ancestor.starts_with(root)).find_map(
                |ancestor| {
                    let manifest = manifests.get(ancestor)?;
                    manifest.contains_key("package").then(|| (ancestor.to_path_buf(), manifest))
                },
            )?;
        let name = manifest.get("package")?.get("name")?.as_str()?.to_owned();
        Some(Self { name, dir, manifest, root, manifests })
    }

    /// Where cargo runs for it: its workspace's root, relative to the project root;
    /// looked for only once it has something to run.
    pub fn cwd(&self) -> String {
        relative(&workspace_root(&self.dir, self.root, self.manifests), self.root)
    }

    /// The binary, example or benchmark whose root file is `path`.
    pub fn main_target(&self, path: &Path) -> Option<Target> {
        cargo_target(path.strip_prefix(&self.dir).ok()?, &self.manifest, &self.name)
    }
}

/// A binary or an example of a Cargo package, run from its workspace's root.
fn rust_main(path: &Path, text: &str, package: &CargoPackage) -> Option<EntryPoint> {
    let line = text.lines().position(is_main_line)?;
    let (name, bin, example) = match package.main_target(path)? {
        Target::Bin(bin) => (bin.clone(), Some(bin), None),
        Target::Example(example) => (example.clone(), None, Some(example)),
        // Its benchmarks are its entry points, and `cargo run` cannot run it.
        Target::Bench(_) => return None,
    };
    let mut configuration = Configuration::new(
        name,
        Kind::Cargo(Cargo {
            package: Some(package.name.clone()),
            bin,
            example,
            ..Cargo::default()
        }),
    );
    configuration.cwd = Some(package.cwd());
    Some(EntryPoint::at(line, configuration))
}

/// `fn main(` at the start of a line, maybe `pub` or `async`.
fn is_main_line(line: &str) -> bool {
    let rest = line.strip_prefix("pub ").unwrap_or(line);
    let rest = rest.strip_prefix("async ").unwrap_or(rest);
    rest.strip_prefix("fn main").is_some_and(|rest| rest.trim_start().starts_with('('))
}

pub(crate) enum Target {
    Bin(String),
    Example(String),
    Bench(String),
}

/// The target whose root file is `file`, relative to the package directory.
fn cargo_target(file: &Path, manifest: &toml::Table, package: &str) -> Option<Target> {
    let declared = |section: &str| {
        manifest
            .get(section)?
            .as_array()?
            .iter()
            .find(|target| target.get("path").and_then(|path| path.as_str()) == file.to_str())?
            .get("name")?
            .as_str()
            .map(str::to_owned)
    };
    let sections = [
        ("bin", Target::Bin as fn(String) -> Target),
        ("example", Target::Example),
        ("bench", Target::Bench),
    ];
    if let Some(target) = sections.iter().find_map(|(section, kind)| declared(section).map(kind)) {
        return Some(target);
    }
    let components: Vec<&str> = file.iter().filter_map(|part| part.to_str()).collect();
    let stem = |name: &str| name.strip_suffix(".rs").map(str::to_owned);
    match components.as_slice() {
        ["src", "main.rs"] => Some(Target::Bin(package.to_owned())),
        ["src", "bin", name] => stem(name).map(Target::Bin),
        ["src", "bin", name, "main.rs"] => Some(Target::Bin((*name).to_owned())),
        ["examples", name] => stem(name).map(Target::Example),
        ["examples", name, "main.rs"] => Some(Target::Example((*name).to_owned())),
        ["benches", name] => stem(name).map(Target::Bench),
        ["benches", name, "main.rs"] => Some(Target::Bench((*name).to_owned())),
        _ => None,
    }
}

/// The root of the Cargo workspace `package_dir` belongs to, else the package's own.
fn workspace_root(package_dir: &Path, root: &Path, manifests: &Manifests) -> PathBuf {
    package_dir
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(root))
        .find(|ancestor| {
            let Some(manifest) = manifests.get(ancestor) else {
                return false;
            };
            let Some(workspace) = manifest.get("workspace") else {
                return false;
            };
            let relative = package_dir.strip_prefix(ancestor).unwrap_or(package_dir);
            let excluded = workspace
                .get("exclude")
                .and_then(|exclude| exclude.as_array())
                .is_some_and(|exclude| {
                    exclude
                        .iter()
                        .filter_map(|path| path.as_str())
                        .any(|path| relative.starts_with(path))
                });
            !excluded
        })
        .unwrap_or(package_dir)
        .to_path_buf()
}

/// A script run whole by Node; declarations, tests and tool configs are left out.
fn script_file(path: &Path, name: &str, root: &Path) -> Option<EntryPoint> {
    let left_out = [".d.ts", ".d.mts", ".d.cts"].iter().any(|suffix| name.ends_with(suffix))
        || [".test.", ".spec.", ".config."].iter().any(|part| name.contains(part));
    if left_out {
        return None;
    }
    Some(EntryPoint::at(0, Configuration::new(name, Kind::Node { file: relative(path, root) })))
}

/// Each script of a `package.json`, at the line that names it.
fn package_scripts(path: &Path, text: &str, root: &Path) -> Vec<EntryPoint> {
    let Ok(package) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(scripts) = package.get("scripts").and_then(|scripts| scripts.as_object()) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let Some(scripts_line) = lines.iter().position(|line| line.contains("\"scripts\"")) else {
        return Vec::new();
    };
    let package_json = relative(path, root);
    scripts
        .keys()
        .filter_map(|script| {
            let key = format!("\"{script}\"");
            let line = scripts_line
                + lines[scripts_line..].iter().position(|line| {
                    line.trim_start()
                        .strip_prefix(&key)
                        .is_some_and(|rest| rest.trim_start().starts_with(':'))
                })?;
            Some(EntryPoint::at(
                line,
                Configuration::new(
                    script_name(path, root, script),
                    Kind::Npm {
                        package_json: package_json.clone(),
                        script: script.clone(),
                        package_manager: None,
                    },
                ),
            ))
        })
        .collect()
}

/// `dev` for a script of the root package, `site: dev` for one in `apps/site`.
pub fn script_name(package_json: &Path, root: &Path, script: &str) -> String {
    match package_json.parent().filter(|directory| *directory != root) {
        Some(directory) => {
            let name = directory.file_name().unwrap_or_default().to_string_lossy();
            format!("{name}: {script}")
        }
        None => script.to_owned(),
    }
}
