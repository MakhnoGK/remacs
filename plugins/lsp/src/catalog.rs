//! The servers' catalog: the built-in servers, as the languages plugin describes them in
//! TOML, with `lsp.toml`'s changes and servers of one's own over them. Each server is
//! kept for the rest of the run once built, as everything that talks to a server holds
//! on to it; only a changed `lsp.toml` builds them again.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use remacs_tools::{InstallSpec, Tool};
use serde::Deserialize;
use serde_json::Value;

use crate::LanguageServer;

/// The built-in catalog's file: its servers in the order they serve a file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuiltIn {
    servers: Vec<NamedSpec>,
}

#[derive(Deserialize)]
struct NamedSpec {
    name: String,
    #[serde(flatten)]
    spec: ServerSpec,
}

/// A server as the catalog or `lsp.toml` describes it; in `lsp.toml`, what a built-in
/// server's entry leaves out stays as built in.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct ServerSpec {
    enabled: Option<bool>,
    /// In `lsp.toml`, for a built-in server, a path (`~/` allowed) or a name on `PATH`
    /// run as it is.
    command: Option<String>,
    args: Option<Vec<String>>,
    /// Major modes and their LSP language ids.
    languages: Option<BTreeMap<String, String>>,
    root_markers: Option<Vec<String>>,
    initialization_options: Option<Value>,
    configuration: Option<Value>,
    pull_diagnostics: Option<bool>,
    reports_loading: Option<bool>,
    node_modules: Option<bool>,
    when: Option<WhenSpec>,
    group: Option<String>,
    install: Option<InstallSpec>,
}

/// When a server serves a project at all, e.g. a linter only where it is set up; any one
/// of these is enough.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct WhenSpec {
    /// Files in the file's directory or above, up to the Git work tree; `*` matches any
    /// part of a name.
    files: Vec<String>,
    /// Packages a `package.json` there depends on; `@scope/*` matches a whole scope.
    dependencies: Vec<String>,
    /// Text a file there contains, by file name.
    contains: BTreeMap<String, String>,
}

/// `WhenSpec` as servers keep it.
#[derive(Debug, Default)]
pub struct When {
    pub files: &'static [&'static str],
    pub dependencies: &'static [&'static str],
    pub contains: &'static [(&'static str, &'static str)],
}

impl When {
    pub fn always(&self) -> bool {
        self.files.is_empty() && self.dependencies.is_empty() && self.contains.is_empty()
    }

    /// Whether one of its conditions holds in `directory` or above, up to the Git work
    /// tree.
    pub fn holds(&self, directory: &Path) -> bool {
        if self.always() {
            return true;
        }
        for ancestor in directory.ancestors() {
            if self.holds_in(ancestor) {
                return true;
            }
            if ancestor.join(".git").exists() {
                return false;
            }
        }
        false
    }

    fn holds_in(&self, directory: &Path) -> bool {
        self.files.iter().any(|file| has_file(directory, file))
            || (!self.dependencies.is_empty() && depends_on(directory, self.dependencies))
            || self.contains.iter().any(|(file, text)| {
                fs::read_to_string(directory.join(file)).is_ok_and(|found| found.contains(text))
            })
    }
}

/// Whether `directory` has a file named `pattern`, where `*` matches any part of a name.
pub(crate) fn has_file(directory: &Path, pattern: &str) -> bool {
    let Some((start, end)) = pattern.split_once('*') else {
        return directory.join(pattern).exists();
    };
    fs::read_dir(directory).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.len() >= start.len() + end.len() && name.starts_with(start) && name.ends_with(end)
        })
    })
}

fn depends_on(directory: &Path, packages: &[&str]) -> bool {
    let Ok(text) = fs::read_to_string(directory.join("package.json")) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    let fields = [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ];
    fields
        .iter()
        .filter_map(|field| manifest.get(field)?.as_object())
        .flat_map(|dependencies| dependencies.keys())
        .any(|name| {
            packages
                .iter()
                .any(|package| match package.strip_suffix('*') {
                    Some(prefix) => name.starts_with(prefix),
                    None => name == package,
                })
        })
}

pub(crate) fn built_in(text: &str) -> Vec<(String, ServerSpec)> {
    let built_in: BuiltIn = toml::from_str(text).expect("the built-in servers are valid");
    built_in
        .servers
        .into_iter()
        .map(|named| (named.name, named.spec))
        .collect()
}

/// The built-in servers with `changes` over them, then the servers only `changes` has;
/// each turned on.
pub(crate) fn merge(
    built_in: Vec<(String, ServerSpec)>,
    changes: &BTreeMap<String, ServerSpec>,
) -> Vec<(String, ServerSpec, bool)> {
    let mut merged: Vec<_> = built_in
        .into_iter()
        .map(|(name, spec)| {
            let changed = changes.get(&name);
            let own_command = changed.is_some_and(|change| change.command.is_some());
            let spec = match changed {
                Some(change) => spec.with(change.clone()),
                None => spec,
            };
            (name, spec, own_command)
        })
        .collect();
    for (name, spec) in changes {
        if !merged.iter().any(|(known, _, _)| known == name) {
            merged.push((name.clone(), spec.clone(), false));
        }
    }
    merged.retain(|(_, spec, _)| spec.enabled != Some(false));
    merged
}

impl ServerSpec {
    fn with(self, change: ServerSpec) -> ServerSpec {
        ServerSpec {
            enabled: change.enabled.or(self.enabled),
            command: change.command.or(self.command),
            args: change.args.or(self.args),
            languages: change.languages.or(self.languages),
            root_markers: change.root_markers.or(self.root_markers),
            initialization_options: change
                .initialization_options
                .or(self.initialization_options),
            configuration: change.configuration.or(self.configuration),
            pull_diagnostics: change.pull_diagnostics.or(self.pull_diagnostics),
            reports_loading: change.reports_loading.or(self.reports_loading),
            node_modules: change.node_modules.or(self.node_modules),
            when: change.when.or(self.when),
            group: change.group.or(self.group),
            install: change.install.or(self.install),
        }
    }

    /// The server it describes, with the tool it is downloaded as. `own_command`: its
    /// command was set in `lsp.toml` and runs as it is.
    pub(crate) fn build(&self, name: &str, own_command: bool) -> Result<LanguageServer, String> {
        let command = self
            .command
            .as_deref()
            .ok_or_else(|| format!("server {name} has no `command`"))?;
        let languages = self
            .languages
            .as_ref()
            .filter(|languages| !languages.is_empty())
            .ok_or_else(|| format!("server {name} serves no `languages`"))?;
        let tool: Option<&'static Tool> = match &self.install {
            Some(install) => Some(install.tool(name, command)?),
            None => None,
        };
        let when = self.when.clone().unwrap_or_default();
        Ok(LanguageServer {
            name: leak(name),
            command: leak(command),
            args: leak_all(self.args.iter().flatten()),
            languages: languages
                .iter()
                .map(|(mode, language)| (leak(mode), leak(language)))
                .collect::<Vec<_>>()
                .leak(),
            root_markers: leak_all(self.root_markers.iter().flatten()),
            initialization_options: self.initialization_options.clone(),
            configuration: self.configuration.clone(),
            pull_diagnostics: self.pull_diagnostics.unwrap_or(false),
            reports_loading: self.reports_loading.unwrap_or(false),
            node_modules: self.node_modules.unwrap_or(false),
            when: When {
                files: leak_all(&when.files),
                dependencies: leak_all(&when.dependencies),
                contains: when
                    .contains
                    .iter()
                    .map(|(file, text)| (leak(file), leak(text)))
                    .collect::<Vec<_>>()
                    .leak(),
            },
            group: self.group.as_deref().map(leak),
            tool,
            own_program: own_command.then(|| &*Box::leak(expand_home(command).into_boxed_path())),
        })
    }
}

fn leak(text: &str) -> &'static str {
    text.to_owned().leak()
}

fn leak_all<'a>(texts: impl IntoIterator<Item = &'a String>) -> &'static [&'static str] {
    texts
        .into_iter()
        .map(|text| leak(text))
        .collect::<Vec<_>>()
        .leak()
}

fn expand_home(command: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match (command.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(command),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn when(files: &[&'static str], dependencies: &[&'static str]) -> When {
        When {
            files: files.to_vec().leak(),
            dependencies: dependencies.to_vec().leak(),
            contains: &[],
        }
    }

    #[test]
    fn conditions_look_up_to_the_work_tree() {
        let root = std::env::temp_dir().join(format!("remacs-when-{}", std::process::id()));
        let app = root.join("app/src");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("tailwind.config.ts"), "").unwrap();
        fs::write(
            root.join("app/package.json"),
            r#"{ "devDependencies": { "@tailwindcss/vite": "4" } }"#,
        )
        .unwrap();
        fs::write(root.join("app/pyproject.toml"), "[tool.ruff]\n").unwrap();

        assert!(when(&[], &[]).holds(&app));
        assert!(when(&["tailwind.config.*"], &[]).holds(&app));
        assert!(!when(&["tailwind.config.js"], &[]).holds(&app));
        assert!(when(&[], &["@tailwindcss/*"]).holds(&app));
        assert!(!when(&[], &["tailwindcss"]).holds(&app));
        let ruff = When {
            contains: vec![("pyproject.toml", "[tool.ruff]")].leak(),
            ..When::default()
        };
        assert!(ruff.holds(&app));
        assert!(!when(&["tailwind.config.*"], &[]).holds(&std::env::temp_dir()));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn lsp_toml_changes_built_in_servers_and_adds_its_own() {
        let built_in = built_in(
            "[[servers]]\n\
             name = \"rust-analyzer\"\n\
             command = \"rust-analyzer\"\n\
             languages = { Rust = \"rust\" }\n\
             [[servers]]\n\
             name = \"eslint\"\n\
             command = \"vscode-eslint-language-server\"\n\
             languages = { JavaScript = \"javascript\" }\n",
        );
        let changes: BTreeMap<String, ServerSpec> = toml::from_str(
            "[rust-analyzer]\n\
             command = \"~/bin/rust-analyzer\"\n\
             [eslint]\n\
             enabled = false\n\
             [serve-d]\n\
             command = \"serve-d\"\n\
             languages = { D = \"d\" }\n\
             install = { from = \"github\", package = \"Pure-D/serve-d\", asset = \"serve-d.tar.xz\" }\n",
        )
        .unwrap();
        let merged = merge(built_in, &changes);
        let names: Vec<_> = merged.iter().map(|(name, _, _)| name.as_str()).collect();
        assert_eq!(names, ["rust-analyzer", "serve-d"]);

        let rust = merged[0].1.build("rust-analyzer", merged[0].2).unwrap();
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert_eq!(
            rust.own_program,
            Some(home.join("bin/rust-analyzer").as_path())
        );
        assert_eq!(rust.languages, [("Rust", "rust")]);
        let serve_d = merged[1].1.build("serve-d", merged[1].2).unwrap();
        assert_eq!(serve_d.own_program, None);
        assert_eq!(serve_d.tool.unwrap().program, "serve-d");

        let incomplete: BTreeMap<String, ServerSpec> =
            toml::from_str("[zls]\ncommand = \"zls\"\n").unwrap();
        let merged = merge(Vec::new(), &incomplete);
        assert_eq!(
            merged[0].1.build("zls", false).err().as_deref(),
            Some("server zls serves no `languages`")
        );
    }
}
