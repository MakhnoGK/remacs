use std::collections::BTreeMap;
use std::time::Duration;

use remacs_core::Section;
use serde::Deserialize;

const CONSOLE_LINES: std::ops::RangeInclusive<usize> = 100..=1_000_000;
/// Milliseconds.
const STOP_TIMEOUTS: std::ops::RangeInclusive<u64> = 0..=60_000;
const ENTRY_POINT_DELAYS: std::ops::RangeInclusive<u64> = 0..=5_000;
const COMPILE_HISTORY: std::ops::RangeInclusive<u64> = 1..=1_000;
const OUTPUT_BACKLOGS: std::ops::RangeInclusive<u64> = 1..=1_024;
const FALLBACK_SHELL: &str = "/bin/sh";

/// `run.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct RunSettings {
    /// Lines a console keeps; older ones are dropped.
    pub console_lines: usize,
    /// Reads of output, 64 KB each, queued for a console busy elsewhere; the program
    /// waits once there are this many. Debugging bounds a build's output and a debug
    /// adapter's messages by it too.
    pub output_backlog: usize,
    /// Milliseconds a process has to end after Stop asks it to, before it is killed.
    pub stop_timeout: u64,
    /// Milliseconds of quiet after an edit before a file's entry points are found again.
    pub entry_point_delay: u64,
    pub typescript: TypeScriptRuntime,
    /// Set for every process run, e.g. so that it prints colors though not to a terminal.
    pub env: BTreeMap<String, String>,
    pub compile: CompileSettings,
}

/// `run.toml` [compile]: how `compile` runs shell commands.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct CompileSettings {
    /// Runs the commands with `-c`, a path or a name; empty, `$SHELL`.
    pub shell: String,
    /// Commands remembered for each project.
    pub history: usize,
    /// Offered in a project that has run none yet: the command of the first whose file
    /// is at the project root, containing `contains` if given.
    pub defaults: Vec<DefaultCommand>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DefaultCommand {
    pub file: String,
    #[serde(default)]
    pub contains: Option<String>,
    pub command: String,
}

impl Default for CompileSettings {
    fn default() -> Self {
        Self {
            shell: String::new(),
            history: 50,
            defaults: [
                ("Cargo.toml", None, "cargo build"),
                ("package.json", Some("\"vite-plus\""), "vp check"),
                ("package.json", None, "npm run build"),
                ("go.mod", None, "go build ./..."),
                ("CMakeLists.txt", None, "cmake --build build"),
                ("Makefile", None, "make -k"),
            ]
            .into_iter()
            .map(|(file, contains, command)| DefaultCommand {
                file: file.to_owned(),
                contains: contains.map(str::to_owned),
                command: command.to_owned(),
            })
            .collect(),
        }
    }
}

impl CompileSettings {
    /// The shell to run commands with: the configured one, else `$SHELL`, else `sh`.
    pub fn shell(&self) -> String {
        Some(self.shell.clone())
            .filter(|shell| !shell.is_empty())
            .or_else(|| std::env::var("SHELL").ok().filter(|shell| !shell.is_empty()))
            .unwrap_or_else(|| FALLBACK_SHELL.to_owned())
    }
}

/// What runs a TypeScript file.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TypeScriptRuntime {
    /// Node when it can strip types itself, from 22.6 on; else `tsx` from `node_modules`.
    #[default]
    Auto,
    Node,
    Tsx,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self {
            console_lines: 10_000,
            output_backlog: 16,
            stop_timeout: 2_000,
            entry_point_delay: 300,
            typescript: TypeScriptRuntime::Auto,
            env: [("CARGO_TERM_COLOR", "always"), ("CLICOLOR_FORCE", "1"), ("FORCE_COLOR", "1")]
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
            compile: CompileSettings::default(),
        }
    }
}

impl RunSettings {
    pub fn stop_timeout(&self) -> Duration {
        Duration::from_millis(self.stop_timeout)
    }

    pub fn entry_point_delay(&self) -> Duration {
        Duration::from_millis(self.entry_point_delay)
    }
}

impl Section for RunSettings {
    const NAME: &str = "run";

    fn check(&self) -> Result<(), String> {
        let within = |value: u64, range: &std::ops::RangeInclusive<u64>, name: &str| {
            if range.contains(&value) {
                Ok(())
            } else {
                Err(format!("{name} must be {} to {}", range.start(), range.end()))
            }
        };
        if !CONSOLE_LINES.contains(&self.console_lines) {
            return Err(format!(
                "console-lines must be {} to {}",
                CONSOLE_LINES.start(),
                CONSOLE_LINES.end()
            ));
        }
        within(self.stop_timeout, &STOP_TIMEOUTS, "stop-timeout")?;
        within(self.output_backlog as u64, &OUTPUT_BACKLOGS, "output-backlog")?;
        within(self.entry_point_delay, &ENTRY_POINT_DELAYS, "entry-point-delay")?;
        within(self.compile.history as u64, &COMPILE_HISTORY, "compile.history")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_default_file_reads_as_the_defaults() {
        remacs_core::assert_default_file::<super::RunSettings>();
    }
}
