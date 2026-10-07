use std::path::Path;
use std::process::Command;

use remacs_core::{CommandResult, Completions, Editor, Prompt};

use crate::chooser;
use crate::configuration::Configuration;
use crate::configuration_store;
use crate::console::{self, link_at, resolve};
use crate::entry_points::EntryPoint;
use crate::links::Target;
use crate::sessions::{self, Session, get, of_window, sessions};
use crate::window::{entry_point_for, window_root};

/// Runs the configuration of the console in the window again; elsewhere the project's
/// chosen one, else the entry point at point, else asks which to run.
pub fn run(editor: &mut Editor) -> CommandResult {
    if let Some(session) = of_window(editor).and_then(|id| get(editor, id)) {
        let (root, configuration) = (session.root.clone(), session.configuration.clone());
        return sessions::run(editor, &root, configuration);
    }
    let root = window_root(editor).ok_or("Not in a project")?;
    let store = configuration_store(editor);
    let error = store.project(&root).1;
    let chosen = store
        .selected(&root)
        .and_then(|name| store.read(&root)?.find(name))
        .cloned();
    if let Some(error) = error {
        editor.set_message(error);
    }
    if let Some(configuration) = chosen {
        return sessions::run(editor, &root, configuration);
    }
    let line = editor.window_buffer().point_line();
    match entry_point_for(editor, line) {
        Some((root, entry)) => run_entry_point(editor, &root, entry),
        None => chooser::choose(editor),
    }
}

/// Runs the entry point at point, or the buffer's first.
pub fn run_at_point(editor: &mut Editor) -> CommandResult {
    let line = editor.window_buffer().point_line();
    let (root, entry) = entry_point_for(editor, line).ok_or("Nothing to run here")?;
    run_entry_point(editor, &root, entry)
}

/// Runs `entry`'s configuration, saved first if the project has none of its kind, and
/// makes it the project's chosen one.
pub fn run_entry_point(editor: &mut Editor, root: &Path, entry: EntryPoint) -> CommandResult {
    let configuration = configuration_store(editor).find_or_add(root, entry.configuration)?;
    run_configuration(editor, root, configuration)
}

/// Makes `configuration` the project's chosen one and runs it.
pub fn run_configuration(
    editor: &mut Editor,
    root: &Path,
    configuration: Configuration,
) -> CommandResult {
    if let Err(error) = configuration_store(editor).select(root, &configuration.name) {
        editor.set_message(format!("Could not keep the chosen configuration: {error}"));
    }
    sessions::run(editor, root, configuration)
}

/// Visits the project's run file at `configuration`, saved first if it is not there yet.
pub fn edit_configuration(
    editor: &mut Editor,
    root: &Path,
    configuration: Configuration,
) -> CommandResult {
    let store = configuration_store(editor);
    let configuration = store.find_or_add(root, configuration)?;
    let (project, _) = store.project(root);
    let path = project
        .path()
        .ok_or("Run configurations are not kept in files")?
        .to_path_buf();
    editor.visit_file(&path)?;
    let name_line = format!("name = {}", toml::Value::String(configuration.name));
    let buffer = editor.window_buffer();
    let text = buffer.text();
    let line = (0..text.line_count()).find(|&line| {
        text.line_range(line)
            .is_some_and(|range| buffer.contents(range).trim() == name_line)
    });
    if let Some(line) = line {
        editor.set_point_at(line, 0);
    }
    Ok(())
}

/// Visits the project's run file.
pub fn edit_configurations(editor: &mut Editor) -> CommandResult {
    let root = window_root(editor).ok_or("Not in a project")?;
    let (project, _) = configuration_store(editor).project(&root);
    let path = project
        .path()
        .ok_or("Run configurations are not kept in files")?
        .to_path_buf();
    editor.visit_file(&path)?;
    Ok(())
}

/// Stops the console's program; elsewhere the chosen configuration's, else the only one
/// running, else asks which.
pub fn stop(editor: &mut Editor) -> CommandResult {
    if let Some(id) = of_window(editor) {
        return stop_session(editor, id);
    }
    let chosen = window_root(editor).and_then(|root| {
        let name = configuration_store(editor).selected(&root)?.to_owned();
        sessions::find(editor, &root, &name)
    });
    if let Some(id) = chosen.filter(|&id| get(editor, id).is_some_and(Session::is_running)) {
        return stop_session(editor, id);
    }
    let running: Vec<(u64, String)> = sessions(editor)
        .list
        .iter()
        .filter(|session| session.is_running())
        .map(|session| (session.id, session.configuration.name.clone()))
        .collect();
    match running.as_slice() {
        [] => Err("Nothing is running".into()),
        [(id, _)] => stop_session(editor, *id),
        _ => chooser::choose_running(editor, running),
    }
}

pub fn stop_session(editor: &mut Editor, id: u64) -> CommandResult {
    if !get(editor, id).is_some_and(Session::is_running) {
        return Err("Not running".into());
    }
    sessions::stop(editor, id);
    Ok(())
}

/// Follows the link at point; elsewhere, sends a line to the program.
pub fn open_link_or_input(editor: &mut Editor) -> CommandResult {
    let id = of_window(editor).ok_or("Not in a console")?;
    let buffer = editor.window_buffer();
    match link_at(buffer, buffer.point()) {
        Some(Target::Url(url)) => {
            Command::new("open").arg(&url).spawn()?;
            Ok(())
        }
        Some(Target::File { path, line, column }) => {
            let file = resolve(buffer, &path).ok_or_else(|| format!("No file {path}"))?;
            editor.visit_file(&file)?;
            editor.set_point_at(
                line.saturating_sub(1),
                column.unwrap_or(1).saturating_sub(1),
            );
            Ok(())
        }
        None => read_input(editor, id),
    }
}

fn read_input(editor: &mut Editor, id: u64) -> CommandResult {
    if !get(editor, id).is_some_and(Session::is_running) {
        return Err("The program is not running".into());
    }
    let prompt = Prompt {
        label: "stdin".into(),
        private: true,
        placeholder: "A line for the program; empty sends just a newline",
        hints: &[("↵", "Send"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        move |editor, line| sessions::send_input(editor, id, &format!("{line}\n")),
    )
}

pub fn send_eof(editor: &mut Editor) -> CommandResult {
    let id = of_window(editor).ok_or("Not in a console")?;
    sessions::close_input(editor, id)
}

pub fn stop_console(editor: &mut Editor) -> CommandResult {
    let id = of_window(editor).ok_or("Not in a console")?;
    stop_session(editor, id)
}

pub fn next_link(editor: &mut Editor) -> CommandResult {
    move_to_link(editor, true)
}

pub fn previous_link(editor: &mut Editor) -> CommandResult {
    move_to_link(editor, false)
}

fn move_to_link(editor: &mut Editor, forward: bool) -> CommandResult {
    let buffer = editor.window_buffer();
    let offset = console::next_link(buffer, buffer.point(), forward).ok_or("No more links")?;
    editor.window_buffer_mut().set_point(offset);
    Ok(())
}

/// Shows the console of the `number`th run, counted from 1 in the order first run.
fn show_session(editor: &mut Editor, number: usize) -> CommandResult {
    let id = sessions(editor)
        .list
        .get(number - 1)
        .map(|session| session.id)
        .ok_or_else(|| format!("No run {number}"))?;
    sessions::show(editor, id);
    Ok(())
}

macro_rules! show_sessions {
    ($($name:ident = $number:literal),*) => {
        $(pub fn $name(editor: &mut Editor) -> CommandResult {
            show_session(editor, $number)
        })*
    };
}

show_sessions!(
    show_session_1 = 1,
    show_session_2 = 2,
    show_session_3 = 3,
    show_session_4 = 4,
    show_session_5 = 5,
    show_session_6 = 6,
    show_session_7 = 7,
    show_session_8 = 8,
    show_session_9 = 9
);
