//! The terminal commands.

use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::vte::ansi::{ClearMode, Handler};
use remacs_config::{config_dir, home};
use remacs_core::{
    Candidate, CommandResult, Completions, Count, Editor, Prompt, filter_candidates,
};
use remacs_files::abbreviate_home;

use crate::copy::{self, CopyMode};
use crate::grid::prompt_lines;
use crate::keys::paste;
use crate::session::Phase;
use crate::settings::Placement;
use crate::terminal::{FALLBACK_SHELL, Terminal};
use crate::terminals::{
    PANE, all, close, create, current, find, find_mut, in_panel, in_window, neighbor,
    restore_window, settings, show_here, show_in_panel, show_in_window, terminals,
};

/// Shows a terminal, or hides it: in the panel, its keys first, or in the window,
/// where the settings open terminals.
pub fn toggle(editor: &mut Editor) -> CommandResult {
    if in_window(editor).is_some() {
        restore_window(editor);
        return Ok(());
    }
    let placement = settings(editor).open_in;
    let in_panel_before = terminals(editor).panel.current;
    let shown_before = in_panel_before
        .filter(|&id| find(editor, id).is_some())
        .or_else(|| {
            editor
                .buffers()
                .iter()
                .find_map(|buffer| buffer.local::<Terminal>().map(|terminal| terminal.id))
        });
    let Some(id) = shown_before else {
        create(editor, placement);
        return Ok(());
    };
    match placement {
        Placement::Buffer => show_in_window(editor, id),
        Placement::Panel if in_panel(editor).is_none() => show_in_panel(editor, id),
        Placement::Panel if !editor.pane_has_keys(PANE) => {
            editor.select_pane(PANE);
            editor.focus_pane(true);
        }
        Placement::Panel => {
            terminals(editor).panel.open = false;
            editor.focus_pane(false);
        }
    }
    Ok(())
}

/// Starts another terminal where the current one is, else where the settings say.
pub fn new_terminal(editor: &mut Editor) -> CommandResult {
    let placement = if in_window(editor).is_some() {
        Placement::Buffer
    } else if in_panel(editor).is_some() {
        Placement::Panel
    } else {
        settings(editor).open_in
    };
    create(editor, placement);
    Ok(())
}

pub fn next(editor: &mut Editor) -> CommandResult {
    cycle(editor, 1)
}

pub fn previous(editor: &mut Editor) -> CommandResult {
    cycle(editor, -1)
}

fn cycle(editor: &mut Editor, step: isize) -> CommandResult {
    let id = current(editor).ok_or("No terminal")?;
    let ids: Vec<u64> = all(editor).iter().map(|terminal| terminal.id).collect();
    let next = neighbor(&ids, id, step).ok_or("No terminal")?;
    show_here(editor, next);
    Ok(())
}

/// Moves the current terminal from the panel to the window, or back.
pub fn maximize(editor: &mut Editor) -> CommandResult {
    if let Some(id) = in_window(editor) {
        show_in_panel(editor, id);
        return Ok(());
    }
    let id = in_panel(editor).ok_or("No terminal in the panel")?;
    show_in_window(editor, id);
    Ok(())
}

pub fn close_current(editor: &mut Editor) -> CommandResult {
    let id = current(editor).ok_or("No terminal")?;
    close(editor, id);
    Ok(())
}

pub fn rename(editor: &mut Editor) -> CommandResult {
    let id = current(editor).ok_or("No terminal")?;
    let label = find(editor, id).map(Terminal::label).unwrap_or_default();
    let prompt = Prompt {
        label: "Rename terminal".into(),
        initial_input: label,
        hints: &[("↵", "Rename"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        move |editor, name| {
            if let Some(terminal) = find_mut(editor, id) {
                let name = name.trim();
                terminal.name = (!name.is_empty()).then(|| name.to_owned());
            }
            crate::terminals::rename_buffers(editor);
            Ok(())
        },
    )
}

/// Chooses a terminal from all of them, shown where the current one is.
pub fn list(editor: &mut Editor) -> CommandResult {
    if all(editor).is_empty() {
        return Err("No terminal".into());
    }
    let prompt = Prompt {
        label: "Terminal".into(),
        placeholder: "Switch to terminal…",
        require_match: true,
        count: Count::Total,
        hints: &[("↵", "Show"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(prompt, candidates, |editor, value| {
        let id = value.parse().map_err(|_| "No such terminal")?;
        show_here(editor, id);
        Ok(())
    })
}

fn candidates(editor: &Editor, input: &str) -> Completions {
    let shown = current(editor);
    let candidates = all(editor)
        .into_iter()
        .map(|terminal| Candidate {
            label: terminal.label(),
            annotation: format!(
                "{} · {}",
                terminal.shell(),
                abbreviate_home(&terminal.directory)
            ),
            tag: (Some(terminal.id) == shown).then_some("current"),
            ..Candidate::new(terminal.id.to_string())
        })
        .collect();
    filter_candidates(input, candidates, false)
}

pub fn copy_mode(editor: &mut Editor) -> CommandResult {
    start_copy(editor, false)
}

pub fn search(editor: &mut Editor) -> CommandResult {
    start_copy(editor, true)
}

fn start_copy(editor: &mut Editor, searching: bool) -> CommandResult {
    let id = current(editor).ok_or("No terminal")?;
    let terminal = find_mut(editor, id).ok_or("No terminal")?;
    let session = terminal
        .session
        .as_ref()
        .ok_or("The terminal has no screen")?;
    let mut term = session.screen.lock();
    copy::enter(&mut term);
    let copying = terminal.copy.get_or_insert_with(CopyMode::default);
    if searching {
        copy::take_key(
            copying,
            &mut term,
            remacs_core::Key::parse("C-r").expect("a key"),
        );
    }
    Ok(())
}

/// Copies the text selected with the mouse.
pub fn copy_selection(editor: &mut Editor) -> CommandResult {
    let id = current(editor).ok_or("No terminal")?;
    let session = find(editor, id)
        .and_then(|terminal| terminal.session.as_ref())
        .ok_or("No terminal")?;
    let mut term = session.screen.lock();
    let text = term.selection_to_string().filter(|text| !text.is_empty());
    term.selection = None;
    drop(term);
    editor.kill_ring.push(text.ok_or("Nothing is selected")?);
    Ok(())
}

/// Types the newest kill, which the system clipboard keeps in step with.
pub fn paste_kill(editor: &mut Editor) -> CommandResult {
    let id = current(editor).ok_or("No terminal")?;
    let text = editor
        .kill_ring
        .newest()
        .ok_or("Nothing to paste")?
        .to_owned();
    let session = find(editor, id)
        .and_then(|terminal| terminal.session.as_ref())
        .ok_or("No terminal")?;
    let bracketed = session
        .screen
        .lock()
        .mode()
        .contains(TermMode::BRACKETED_PASTE);
    session.write(paste(&text, bracketed));
    Ok(())
}

/// Clears the scrollback and the screen; the shell, if waiting, draws its prompt again.
pub fn clear(editor: &mut Editor) -> CommandResult {
    let id = current(editor).ok_or("No terminal")?;
    let session = find(editor, id)
        .and_then(|terminal| terminal.session.as_ref())
        .ok_or("No terminal")?;
    {
        let mut term = session.screen.lock();
        term.clear_screen(ClearMode::Saved);
        term.scroll_display(Scroll::Bottom);
    }
    if session.is_idle() {
        session.write(*b"\x0c");
    }
    Ok(())
}

pub fn previous_prompt(editor: &mut Editor) -> CommandResult {
    jump_to_prompt(editor, false)
}

pub fn next_prompt(editor: &mut Editor) -> CommandResult {
    jump_to_prompt(editor, true)
}

/// Scrolls the prompt above or below the top row to the top; prompts are known when the
/// shell reports them, as fish does.
fn jump_to_prompt(editor: &mut Editor, forward: bool) -> CommandResult {
    let id = current(editor).ok_or("No terminal")?;
    let session = find(editor, id)
        .and_then(|terminal| terminal.session.as_ref())
        .ok_or("No terminal")?;
    let mut term = session.screen.lock();
    let prompts = prompt_lines(&term);
    if prompts.is_empty() {
        return Err("The shell reports no prompts (OSC 133)".into());
    }
    let top = -(term.grid().display_offset() as i32);
    let target = if forward {
        prompts.keys().find(|&&line| line > top).copied()
    } else {
        prompts.keys().rev().find(|&&line| line < top).copied()
    };
    let target = target.ok_or(if forward {
        "No later command"
    } else {
        "No earlier command"
    })?;
    // The view cannot go below the screen: a prompt on it is shown with the screen.
    let line = target.min(0);
    term.scroll_display(Scroll::Delta(top - line));
    Ok(())
}

/// Starts the ended terminal's shell again, where it last was.
pub fn restart(editor: &mut Editor) -> CommandResult {
    start_again(editor, None)
}

/// Starts the fallback shell in the terminal whose shell could not start, this once.
pub fn start_fallback(editor: &mut Editor) -> CommandResult {
    start_again(editor, Some(FALLBACK_SHELL))
}

fn start_again(editor: &mut Editor, program: Option<&str>) -> CommandResult {
    let id = current(editor).ok_or("No terminal")?;
    let settings = settings(editor);
    let notifier = editor.notifier();
    let terminal = find_mut(editor, id).ok_or("No terminal")?;
    if !matches!(terminal.phase, Phase::Exited(_) | Phase::Failed(_)) {
        return Err("The terminal is still running".into());
    }
    if let Some(program) = program {
        terminal.program = program.to_owned();
    } else if matches!(terminal.phase, Phase::Failed(_)) {
        terminal.program = crate::terminal::configured_shell(&settings);
    }
    terminal.start(&settings, notifier);
    Ok(())
}

pub fn close_ended(editor: &mut Editor) -> CommandResult {
    close_current(editor)
}

/// Visits `terminal.toml` at its `shell` line.
pub fn open_settings(editor: &mut Editor) -> CommandResult {
    let home = home().ok_or("No home directory")?;
    let path = config_dir(&home).join("terminal.toml");
    if in_window(editor).is_none() {
        editor.focus_pane(false);
    }
    editor
        .visit_file(&path)
        .map_err(|error| error.to_string())?;
    let buffer = editor.window_buffer();
    let text = buffer.text();
    let line = (0..text.line_count())
        .find(|&line| {
            text.line_range(line)
                .is_some_and(|range| buffer.contents(range).trim_start().starts_with("shell"))
        })
        .unwrap_or(0);
    editor.set_point_at(line, 0);
    Ok(())
}
