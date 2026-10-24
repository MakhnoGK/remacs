//! The programs remacs can download, and where from. Debug adapters are built in;
//! language servers come from the servers' catalog, as `InstallSpec`s.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    DebugAdapter,
    LanguageServer,
    /// A tree-sitter grammar built to WebAssembly (ADR 0087).
    Grammar,
}

impl ToolKind {
    pub fn group(self) -> &'static str {
        match self {
            ToolKind::DebugAdapter => "Debug adapters",
            ToolKind::LanguageServer => "Language servers",
            ToolKind::Grammar => "Grammars",
        }
    }
}

/// How a GitHub release's asset is unpacked, told by its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Archive {
    Zip,
    TarGz,
    TarXz,
    /// One gzipped program.
    Gzip,
    /// The program itself.
    Program,
}

impl Archive {
    pub fn of(asset: &str) -> Self {
        let has = |extension: &str| asset.ends_with(extension);
        if has(".zip") || has(".vsix") {
            Archive::Zip
        } else if has(".tar.gz") || has(".tgz") {
            Archive::TarGz
        } else if has(".tar.xz") {
            Archive::TarXz
        } else if has(".gz") {
            Archive::Gzip
        } else {
            Archive::Program
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A release asset of a GitHub repository.
    GitHub {
        /// `owner/name`.
        repository: &'static str,
        /// Before the version in a release's tag, e.g. `v`; it also tells a tool's
        /// releases from others' in a repository that releases several.
        tag_prefix: &'static str,
        /// The release's asset for this Mac, `{tag}`, `{version}` and what `fill` fills
        /// in.
        asset: &'static str,
    },
    /// A package that one of the installers (npm, go, pip…) installs. Tools of one
    /// package share one install.
    Package {
        installer: &'static str,
        package: &'static str,
    },
}

/// A program downloaded when it is first wanted.
#[derive(Debug, PartialEq, Eq)]
pub struct Tool {
    pub name: &'static str,
    pub kind: ToolKind,
    pub source: Source,
    /// From GitHub, the program relative to where the asset is unpacked, `{version}` and
    /// what `fill` fills in; from an installer, the program's name, which the installer
    /// says where to find.
    pub program: &'static str,
    /// What `{arch}` stands for on Apple silicon and on Intel, where the tool's builds
    /// name them their own way.
    pub arch: Option<(&'static str, &'static str)>,
    /// Programs it runs with, such as Java, which must be on `PATH` to offer it.
    pub needs: &'static [&'static str],
    /// The only release that will do, unless `tools.toml` pins another: a grammar's,
    /// which remacs's queries are written for.
    pub version: Option<&'static str>,
}

impl Tool {
    pub fn asset_name(&self, tag: &str) -> String {
        match self.source {
            Source::GitHub { asset, .. } => self
                .fill(asset)
                .replace("{tag}", tag)
                .replace("{version}", &self.version_of(tag)),
            Source::Package { package, .. } => package.to_owned(),
        }
    }

    /// `fill`, with its own name for `{arch}`.
    pub fn fill(&self, template: &str) -> String {
        let template = match self.arch {
            Some((apple, intel)) => {
                template.replace("{arch}", if APPLE_SILICON { apple } else { intel })
            }
            None => template.to_owned(),
        };
        fill(&template)
    }

    pub fn version_of(&self, tag: &str) -> String {
        let prefix = match self.source {
            Source::GitHub { tag_prefix, .. } => tag_prefix,
            Source::Package { .. } => "",
        };
        tag.strip_prefix(prefix).unwrap_or(tag).to_owned()
    }

    pub fn tag_of(&self, version: &str) -> String {
        match self.source {
            Source::GitHub { tag_prefix, .. } => format!("{tag_prefix}{version}"),
            Source::Package { .. } => version.to_owned(),
        }
    }

    /// The directory its versions are installed in, under the tools' directory.
    pub fn home(&self) -> String {
        match self.source {
            Source::GitHub { .. } => self.name.to_owned(),
            // A URL names no package, so its tool does.
            Source::Package { installer, package } if package.contains("://") => {
                format!("{installer}-{}", self.name)
            }
            Source::Package { installer, package } => {
                format!("{installer}-{}", package.replace(['/', '@'], "+"))
            }
        }
    }

    /// Where it comes from, as the Tools panel says.
    pub fn origin(&self) -> &'static str {
        match self.source {
            Source::GitHub { .. } => "GitHub releases",
            Source::Package { installer, .. } => installer,
        }
    }
}

const APPLE_SILICON: bool = cfg!(target_arch = "aarch64");

/// Fills in what tells this Mac's builds apart, as builds name them: `{arch}` (`arm64`,
/// `x64`), `{cpu}` (`aarch64`, `x86_64`), `{goarch}` (`arm64`, `amd64`) and `{target}`
/// (`aarch64-apple-darwin`, `x86_64-apple-darwin`).
pub fn fill(template: &str) -> String {
    let (arch, cpu, goarch) = if APPLE_SILICON {
        ("arm64", "aarch64", "arm64")
    } else {
        ("x64", "x86_64", "amd64")
    };
    template
        .replace("{arch}", arch)
        .replace("{cpu}", cpu)
        .replace("{goarch}", goarch)
        .replace("{target}", &format!("{cpu}-apple-darwin"))
}

/// Where a language server is downloaded from, as the servers' catalog says:
/// `{ from = "github", package = "owner/name", asset = "…" }`, or
/// `{ from = "npm", package = "pyright", program = "pyright-langserver" }` for any
/// installer.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct InstallSpec {
    pub from: String,
    pub package: String,
    /// GitHub only.
    pub asset: Option<String>,
    /// GitHub only.
    #[serde(default)]
    pub tag_prefix: String,
    /// Else the server's command.
    pub program: Option<String>,
    /// What `{arch}` stands for on Apple silicon, then on Intel.
    pub arch: Option<[String; 2]>,
    #[serde(default)]
    pub needs: Vec<String>,
}

impl InstallSpec {
    /// The tool named `name` it describes, kept for the rest of the run as tools are
    /// shared with background installs; `command` is its program unless it names one.
    pub fn tool(&self, name: &str, command: &str) -> Result<&'static Tool, String> {
        let leak = |text: &str| -> &'static str { text.to_owned().leak() };
        let source = if self.from == "github" {
            let asset = self
                .asset
                .as_deref()
                .ok_or_else(|| format!("{name}: a GitHub download names its `asset`"))?;
            Source::GitHub {
                repository: leak(&self.package),
                tag_prefix: leak(&self.tag_prefix),
                asset: leak(asset),
            }
        } else {
            if self.asset.is_some() || !self.tag_prefix.is_empty() {
                return Err(format!("{name}: only GitHub downloads have an asset"));
            }
            Source::Package {
                installer: leak(&self.from),
                package: leak(&self.package),
            }
        };
        Ok(Box::leak(Box::new(Tool {
            name: leak(name),
            kind: ToolKind::LanguageServer,
            source,
            program: leak(self.program.as_deref().unwrap_or(command)),
            arch: self
                .arch
                .as_ref()
                .map(|[apple, intel]| (leak(apple), leak(intel))),
            needs: self
                .needs
                .iter()
                .map(|program| leak(program))
                .collect::<Vec<_>>()
                .leak(),
            version: None,
        })))
    }
}

/// Debugs Rust, C and C++ through LLDB.
pub const CODELLDB: Tool = Tool {
    name: "codelldb",
    kind: ToolKind::DebugAdapter,
    source: Source::GitHub {
        repository: "vadimcn/codelldb",
        tag_prefix: "v",
        asset: "codelldb-darwin-{arch}.vsix",
    },
    program: "extension/adapter/codelldb",
    arch: None,
    needs: &[],
    version: None,
};

/// Debugs JavaScript and TypeScript on Node; run by Node itself.
pub const JS_DEBUG: Tool = Tool {
    name: "js-debug",
    kind: ToolKind::DebugAdapter,
    source: Source::GitHub {
        repository: "microsoft/vscode-js-debug",
        tag_prefix: "v",
        asset: "js-debug-dap-{tag}.tar.gz",
    },
    program: "js-debug/src/dapDebugServer.js",
    arch: None,
    needs: &[],
    version: None,
};

pub const DEBUG_ADAPTERS: &[&Tool] = &[&CODELLDB, &JS_DEBUG];

/// Orders versions by their numbers, e.g. `1.9.0` before `1.11.4`, and dates.
pub fn version_key(version: &str) -> Vec<u64> {
    version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().unwrap_or(u64::MAX))
        .collect()
}
