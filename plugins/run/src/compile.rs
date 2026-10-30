//! `compile`, as in Emacs: a shell command line run from the project's root nearest the
//! file, or a Dired buffer's directory, its output in that directory's compile console,
//! whose `file:line`s `compile-next-error` visits in turn.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use remacs_core::{Candidate, CommandResult, Completions, Editor, Prompt, filter_candidates};
use remacs_dired::DIRED_MODE;
use remacs_files::links::follow;
use remacs_files::{abbreviate_home, with_slash};

use crate::configuration::{Configuration, Kind};
use crate::console::{Console, next_location};
use crate::sessions::{self, Session, Sessions, find_compile, latest, sessions};
use crate::settings::CompileSettings;
use crate::window::window_root;

pub const COMPILE: &str = "compile";
pub const RECOMPILE: &str = "recompile";
pub const NEXT_ERROR: &str = "compile-next-error";
pub const PREVIOUS_ERROR: &str = "compile-previous-error";
/// `next-error` visits the console's locations from a run or compile, or from visiting
/// one, until the problems of a buffer are gone to.
pub const OUTPUT: &str = "compile output";

const HINTS: &[(&str, &str)] =
    &[("↵", "Run"), ("TAB", "Edit"), ("M-↵", "Run as typed"), ("C-g", "Cancel")];

/// The command lines compiled so far, newest first, by project root; kept in a state
/// file across sessions.
#[derive(Default)]
pub struct History {
    commands: BTreeMap<PathBuf, Vec<String>>,
    file: Option<PathBuf>,
}

impl History {
    /// Reads the commands kept in `file`, and writes them to it from now on.
    pub fn keep_in(&mut self, file: PathBuf) {
        self.commands = fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        self.file = Some(file);
    }

    pub fn of(&self, root: &Path) -> &[String] {
        self.commands.get(root).map_or(&[], Vec::as_slice)
    }

    fn add(&mut self, root: &Path, command: &str, limit: usize) -> io::Result<()> {
        let commands = self.commands.entry(root.to_path_buf()).or_default();
        commands.retain(|earlier| earlier != command);
        commands.insert(0, command.to_owned());
        commands.truncate(limit);
        let Some(file) = &self.file else {
            return Ok(());
        };
        if let Some(directory) = file.parent() {
            fs::create_dir_all(directory)?;
        }
        let json = serde_json::to_string_pretty(&self.commands).map_err(io::Error::other)?;
        fs::write(file, json)
    }
}

pub fn history(editor: &mut Editor) -> &mut History {
    editor.plugin_state()
}

fn settings(editor: &mut Editor) -> CompileSettings {
    sessions(editor).settings.compile.clone()
}

/// Where `compile` runs: a console's own directory; a Dired buffer's; else the nearest
/// directory within the project with the file of one of [compile] `defaults`, else the
/// project root.
pub fn compile_directory(editor: &Editor) -> Option<PathBuf> {
    let buffer = editor.window_buffer();
    let directory = buffer.default_directory().map(canonical);
    if buffer.mode() == DIRED_MODE {
        return directory;
    }
    let root = window_root(editor)?;
    if buffer.local::<Console>().is_some() {
        return Some(root);
    }
    let settings =
        editor.find_plugin_state::<Sessions>().map(|sessions| &sessions.settings.compile);
    let nearest = directory.zip(settings).and_then(|(directory, settings)| {
        directory
            .ancestors()
            .take_while(|ancestor| ancestor.starts_with(&root))
            .find(|ancestor| default_command(ancestor, settings).is_some())
            .map(Path::to_path_buf)
    });
    Some(nearest.unwrap_or(root))
}

fn canonical(directory: &Path) -> PathBuf {
    directory.canonicalize().unwrap_or_else(|_| directory.to_path_buf())
}

/// Asks for a command line, the directory's last one first, and runs it there in its
/// compile console.
pub fn compile(editor: &mut Editor) -> CommandResult {
    let root = compile_directory(editor).ok_or("Not in a project")?;
    let commands = history(editor).of(&root).to_vec();
    let suggested = default_command(&root, &settings(editor));
    let default = commands.first().cloned().or_else(|| suggested.clone());
    let prompt = Prompt {
        label: "Compile".into(),
        private: true,
        directory: with_slash(&abbreviate_home(&root)),
        placeholder: "A shell command, run in this directory",
        default,
        hints: HINTS,
        ..Prompt::default()
    };
    let complete =
        move |_: &Editor, input: &str| candidates(&commands, suggested.as_deref(), input);
    editor.read(prompt, complete, move |editor, command| run(editor, &root, &command))
}

/// The commands run before, best matches first; what is typed comes first, so that the
/// answer is it unless a command is chosen.
pub fn candidates(commands: &[String], suggested: Option<&str>, input: &str) -> Completions {
    let input = input.trim();
    let mut known: Vec<Candidate> = commands.iter().map(Candidate::new).collect();
    if let Some(suggested) =
        suggested.filter(|suggested| !commands.iter().any(|command| command == suggested))
    {
        known.push(Candidate { annotation: "suggested".into(), ..Candidate::new(suggested) });
    }
    if input.is_empty() {
        return known.into();
    }
    known.retain(|candidate| candidate.value != input);
    let mut completions = filter_candidates(input, known, true);
    completions.candidates.insert(0, Candidate::new(input));
    completions.total += 1;
    completions
}

/// The command of the first of `run.toml`'s defaults whose file is at the project root,
/// with the text it should contain.
pub fn default_command(root: &Path, settings: &CompileSettings) -> Option<String> {
    settings
        .defaults
        .iter()
        .find(|default| {
            let file = root.join(&default.file);
            match &default.contains {
                Some(text) => fs::read_to_string(&file).is_ok_and(|found| found.contains(text)),
                None => file.exists(),
            }
        })
        .map(|default| default.command.clone())
}

/// Runs `command` from `root` as the project's compile, and remembers it.
pub fn run(editor: &mut Editor, root: &Path, command: &str) -> CommandResult {
    let command = command.trim();
    if command.is_empty() {
        return Err("No command to run".into());
    }
    let limit = settings(editor).history;
    if let Err(error) = history(editor).add(root, command, limit) {
        editor.set_error(format!("Could not keep the command: {error}"));
    }
    let configuration = Configuration::new(command, Kind::Shell { command: command.to_owned() });
    sessions::compile(editor, root, configuration)
}

/// Runs the project's compile again, else its last command, else asks for one.
pub fn recompile(editor: &mut Editor) -> CommandResult {
    let root = compile_directory(editor).ok_or("Not in a project")?;
    if let Some(id) = find_compile(editor, &root) {
        return sessions::rerun(editor, id, None);
    }
    match history(editor).of(&root).first().cloned() {
        Some(command) => run(editor, &root, &command),
        None => compile(editor),
    }
}

pub fn next_error(editor: &mut Editor) -> CommandResult {
    visit_location(editor, true)
}

pub fn previous_error(editor: &mut Editor) -> CommandResult {
    visit_location(editor, false)
}

/// Visits the next `file:line` of the output, or the previous one: after point in a
/// console, elsewhere after the one visited last in the project's compile console, else
/// in the console of the latest run.
fn visit_location(editor: &mut Editor, forward: bool) -> CommandResult {
    let in_window = editor.window_buffer().local::<Console>().is_some();
    let name = if in_window {
        editor.window_buffer().name().to_owned()
    } else {
        output_console(editor).ok_or("Nothing compiled or run yet")?
    };
    let buffer =
        editor.buffers().iter().find(|buffer| buffer.name() == name).ok_or("No such console")?;
    let console = buffer.local::<Console>().ok_or("Not a console")?;
    let offset = if in_window { Some(buffer.point()) } else { console.location };
    let (start, target) = next_location(buffer, offset, forward).ok_or(if forward {
        "No more locations"
    } else {
        "No earlier locations"
    })?;
    let directories = console.directories.to_vec();
    editor.plugin_state::<Visited>().0 = Some(console.session);
    let buffer = editor.buffer_named_mut(&name).expect("found by name");
    buffer.set_point(start);
    if let Some(console) = buffer.local_mut::<Console>() {
        console.location = Some(start);
    }
    editor.set_next_error_source(Some(OUTPUT));
    follow(editor, &target, &directories, in_window)
}

/// The session whose console `compile-next-error` went through last.
#[derive(Default)]
struct Visited(Option<u64>);

/// The console of the window buffer's directory's compile, else the one visited last,
/// so that a location in another package goes on there, else of the latest run.
fn output_console(editor: &Editor) -> Option<String> {
    let compiled = compile_directory(editor).and_then(|root| find_compile(editor, &root));
    let visited = editor.find_plugin_state::<Visited>().and_then(|visited| visited.0);
    [compiled, visited, latest(editor)]
        .into_iter()
        .flatten()
        .find_map(|id| Session::buffer_name(editor, id))
}

pub fn has_output(editor: &Editor) -> bool {
    latest(editor).is_some()
}
