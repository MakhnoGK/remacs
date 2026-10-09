//! Which debug adapter debugs a configuration, what is done before it starts, and what it
//! is asked to launch.

use std::env;
use std::path::Path;

use remacs_run::{Configuration, Kind, Launch};
use serde_json::{Map, Value, json};

use crate::prepare::{CargoBuild, Plan, Target};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapter {
    /// Rust, C and C++, through LLDB.
    CodeLldb,
    /// JavaScript and TypeScript on Node.
    JsDebug,
}

impl Adapter {
    /// The tool that is the adapter.
    pub fn tool(self) -> &'static str {
        match self {
            Adapter::CodeLldb => remacs_tools::CODELLDB.name,
            Adapter::JsDebug => remacs_tools::JS_DEBUG.name,
        }
    }

    /// What the adapter is told it is, as VS Code's debug type.
    pub fn id(self) -> &'static str {
        match self {
            Adapter::CodeLldb => "lldb",
            Adapter::JsDebug => "pwa-node",
        }
    }

    /// The `launch` request's arguments for `target`.
    pub fn launch_arguments(self, name: &str, target: &Target, rust: bool) -> Value {
        let env: Map<String, Value> = target
            .env
            .iter()
            .map(|(name, value)| (name.clone(), value.clone().into()))
            .collect();
        match self {
            Adapter::CodeLldb => {
                let mut arguments = json!({
                    "type": self.id(),
                    "request": "launch",
                    "name": name,
                    "program": target.program,
                    "args": target.args,
                    "cwd": target.cwd,
                    "env": env,
                    "terminal": "console",
                    "stopOnEntry": false,
                });
                if rust {
                    arguments["sourceLanguages"] = json!(["rust"]);
                }
                arguments
            }
            Adapter::JsDebug => json!({
                "type": self.id(),
                "request": "launch",
                "name": name,
                "runtimeExecutable": target.program,
                "runtimeArgs": target.args,
                "cwd": target.cwd,
                "env": env,
                "console": "internalConsole",
                "outputCapture": "std",
                "skipFiles": ["<node_internals>/**"],
            }),
        }
    }
}

/// How `configuration`, run as `launch`, is debugged.
pub fn plan(configuration: &Configuration, launch: &Launch) -> Result<(Adapter, Plan), String> {
    let mut target = Target {
        program: launch.program.clone(),
        args: launch.args.clone(),
        cwd: launch.cwd.clone(),
        env: launch.env.clone(),
    };
    let mut cargo_build = None;
    let adapter = match &configuration.kind {
        Kind::Cargo(cargo) => {
            let tests = cargo.runs_tests();
            if !tests && cargo.command != "run" {
                return Err(format!(
                    "cargo {} cannot be debugged; only cargo run and cargo test",
                    cargo.command
                ));
            }
            let mut args = cargo.selection();
            args.extend(cargo.cargo_args.iter().cloned());
            cargo_build = Some(CargoBuild { tests, args });
            target.program = String::new();
            target.args = cargo
                .filter
                .iter()
                .filter(|_| tests)
                .chain(&configuration.args)
                .cloned()
                .collect();
            Adapter::CodeLldb
        }
        Kind::Executable { .. } => {
            target.program = on_path(&target.program)
                .ok_or_else(|| format!("{} is not on PATH", target.program))?;
            Adapter::CodeLldb
        }
        Kind::Node { .. } | Kind::Npm { .. } => Adapter::JsDebug,
    };
    let plan = Plan {
        before: launch.before.clone(),
        cargo_build,
        target,
    };
    Ok((adapter, plan))
}

/// `program` as a path: itself if it has a slash, else where `PATH` finds it.
fn on_path(program: &str) -> Option<String> {
    if program.contains('/') {
        return Some(program.to_owned());
    }
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|path| is_executable(path))
        .map(|path| path.display().to_string())
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}
