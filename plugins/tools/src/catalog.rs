//! The programs remacs can download, and where from.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind {
    DebugAdapter,
    LanguageServer,
}

impl ToolKind {
    pub fn group(self) -> &'static str {
        match self {
            ToolKind::DebugAdapter => "Debug adapters",
            ToolKind::LanguageServer => "Language servers",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Archive {
    Zip,
    TarGz,
}

/// A program downloaded on first use from the releases of a GitHub repository.
#[derive(Debug, PartialEq, Eq)]
pub struct Tool {
    pub name: &'static str,
    pub kind: ToolKind,
    /// `owner/name`.
    pub repository: &'static str,
    /// Before the version in a release's tag, e.g. `v`.
    pub tag_prefix: &'static str,
    /// The release's asset for this Mac, `{tag}` and `{arch}` (`arm64` or `x64`) filled in.
    pub asset: &'static str,
    pub archive: Archive,
    /// The program, relative to where the asset is unpacked.
    pub program: &'static str,
}

impl Tool {
    pub fn asset_name(&self, tag: &str) -> String {
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        };
        self.asset.replace("{tag}", tag).replace("{arch}", arch)
    }

    pub fn version_of(&self, tag: &str) -> String {
        tag.strip_prefix(self.tag_prefix).unwrap_or(tag).to_owned()
    }
}

/// Debugs Rust, C and C++ through LLDB.
pub const CODELLDB: Tool = Tool {
    name: "codelldb",
    kind: ToolKind::DebugAdapter,
    repository: "vadimcn/codelldb",
    tag_prefix: "v",
    asset: "codelldb-darwin-{arch}.vsix",
    archive: Archive::Zip,
    program: "extension/adapter/codelldb",
};

/// Debugs JavaScript and TypeScript on Node; run by Node itself.
pub const JS_DEBUG: Tool = Tool {
    name: "js-debug",
    kind: ToolKind::DebugAdapter,
    repository: "microsoft/vscode-js-debug",
    tag_prefix: "v",
    asset: "js-debug-dap-{tag}.tar.gz",
    archive: Archive::TarGz,
    program: "js-debug/src/dapDebugServer.js",
};

pub const TOOLS: &[&Tool] = &[&CODELLDB, &JS_DEBUG];

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().copied().find(|tool| tool.name == name)
}

/// Orders versions by their numbers, e.g. `1.9.0` before `1.11.4`, and dates.
pub fn version_key(version: &str) -> Vec<u64> {
    version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse().unwrap_or(u64::MAX))
        .collect()
}
