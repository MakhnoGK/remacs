use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use remacs_core::{
    Editor, Key, Screen, ScreenButton, ScreenMouse, ScreenPointer, ScreenSize, Settings,
    parse_sequence,
};
use remacs_editing::EditingPlugin;
use remacs_files::FilesPlugin;
use remacs_minibuffer::MinibufferPlugin;

use super::*;
use crate::terminals::{in_panel, in_window};

const WAIT: Duration = Duration::from_secs(10);
const SIZE: ScreenSize = ScreenSize {
    columns: 80,
    rows: 12,
};

fn editor(terminal_toml: &str) -> Editor {
    let mut editor = Editor::new(&[
        &EditingPlugin,
        &MinibufferPlugin,
        &FilesPlugin,
        &TerminalPlugin,
    ]);
    editor.set_settings(Settings::from_files([(
        PathBuf::from("terminal.toml"),
        format!("directory = \"home\"\n{terminal_toml}"),
    )]));
    editor
}

const SH: &str = "shell = \"/bin/sh\"\nargs = []\n";

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        let key = match character {
            ' ' => "SPC".to_owned(),
            '\n' => "RET".to_owned(),
            other => other.to_string(),
        };
        press(editor, &key);
    }
}

/// The terminal shown: the window's, else the panel's.
fn shown(editor: &Editor) -> Option<Screen> {
    let id = in_window(editor).or_else(|| in_panel(editor))?;
    let name = terminals::buffer_name(editor, id)?;
    editor.screen(&name, SIZE)
}

fn text(screen: &Screen) -> String {
    screen
        .rows
        .iter()
        .map(|row| {
            row.runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Refreshes as the UI would until `done` holds of the shown screen.
fn wait_for(editor: &mut Editor, what: &str, done: impl Fn(&Screen) -> bool) {
    let start = Instant::now();
    loop {
        editor.refresh();
        if let Some(screen) = shown(editor)
            && done(&screen)
        {
            return;
        }
        assert!(
            start.elapsed() < WAIT,
            "gave up waiting for {what}; screen:\n{}",
            shown(editor)
                .map(|screen| text(&screen))
                .unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_text(editor: &mut Editor, wanted: &str) {
    wait_for(editor, wanted, |screen| text(screen).contains(wanted));
}

fn window_text(editor: &Editor) -> String {
    let buffer = editor.window_buffer();
    buffer.contents(0..buffer.text().len())
}

#[test]
fn a_terminal_in_the_panel_takes_the_keys_until_they_go_back_to_the_text() {
    let mut editor = editor(SH);
    press(&mut editor, "C-`");
    assert_eq!(editor.shown_pane(), Some(PANE));
    assert!(editor.pane_has_keys(PANE));
    wait_for(&mut editor, "the prompt", |screen| screen.notice.is_none());
    type_text(&mut editor, "echo hi-$((40 + 2))\n");
    wait_for_text(&mut editor, "hi-42");
    assert_eq!(window_text(&editor), "", "nothing typed reached the text");
    // C-c reaches the shell, not remacs's prefix.
    type_text(&mut editor, "sleep 30");
    wait_for_text(&mut editor, "sleep 30");
    press(&mut editor, "C-c");
    // dash drops a line typed before it is back at its prompt along with the interrupted one.
    wait_for(&mut editor, "the prompt after C-c", |screen| {
        text(screen).trim_end().ends_with(['$', '#'])
    });
    type_text(&mut editor, "echo after\n");
    wait_for_text(&mut editor, "\nafter");

    press(&mut editor, "C-x o");
    assert!(!editor.pane_has_keys(PANE));
    press(&mut editor, "x");
    assert_eq!(window_text(&editor), "x");
    // The panel stays; C-` gives it the keys again, then hides it.
    assert_eq!(editor.shown_pane(), Some(PANE));
    press(&mut editor, "C-`");
    assert!(editor.pane_has_keys(PANE));
    press(&mut editor, "C-`");
    assert_eq!(editor.shown_pane(), None);
}

#[test]
fn a_terminal_moves_between_the_panel_and_the_window() {
    let mut editor = editor(SH);
    let code = editor.window_buffer().name().to_owned();
    press(&mut editor, "C-`");
    press(&mut editor, "s-RET");
    assert!(editor.window_buffer().name().starts_with("*term: "));
    assert_eq!(editor.shown_pane(), None);
    let controls = editor.text_controls().expect("chips over the terminal");
    assert_eq!(controls.tabs.map(|tabs| tabs.tabs.len()), Some(1));
    wait_for(&mut editor, "the prompt", |screen| screen.notice.is_none());
    type_text(&mut editor, "echo in-window\n");
    wait_for_text(&mut editor, "\nin-window");

    // Back to the code, the terminal is not left in the panel too.
    let terminal = editor.window_buffer().name().to_owned();
    editor.switch_to_buffer(&code);
    assert_eq!(editor.shown_pane(), None);
    editor.switch_to_buffer(&terminal);

    press(&mut editor, "s-RET");
    assert_eq!(editor.window_buffer().name(), code);
    assert_eq!(editor.shown_pane(), Some(PANE));
    assert!(editor.pane_has_keys(PANE));

    // A second terminal; ⌘{ and ⌘} go through both.
    press(&mut editor, "s-t");
    let second = in_panel(&editor).unwrap();
    press(&mut editor, "s-}");
    assert_ne!(in_panel(&editor), Some(second));
    press(&mut editor, "s-{");
    assert_eq!(in_panel(&editor), Some(second));
    assert_eq!(editor.pane().and_then(|pane| pane.count), Some(2));
}

/// Refreshes until quitting would, or would not, end what the terminal runs.
fn wait_for_running(editor: &mut Editor, running: bool) {
    let start = Instant::now();
    while editor.running_programs().is_empty() == running {
        assert!(
            start.elapsed() < WAIT,
            "gave up waiting for running = {running}"
        );
        editor.refresh();
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn quitting_asks_only_while_a_terminal_runs_more_than_its_shell() {
    let mut editor = editor(SH);
    press(&mut editor, "C-`");
    wait_for(&mut editor, "the prompt", |screen| screen.notice.is_none());
    assert_eq!(editor.running_programs(), Vec::<String>::new());

    type_text(&mut editor, "sleep 30\n");
    wait_for_running(&mut editor, true);
    let name = terminals::buffer_name(&editor, in_panel(&editor).unwrap()).unwrap();
    assert_eq!(editor.running_programs(), [name]);

    press(&mut editor, "C-c");
    wait_for_running(&mut editor, false);
}

#[test]
fn an_ended_shell_restarts_or_closes() {
    let mut editor = editor(SH);
    press(&mut editor, "C-`");
    wait_for(&mut editor, "the prompt", |screen| screen.notice.is_none());
    type_text(&mut editor, "exit 3\n");
    wait_for(&mut editor, "the exit", |screen| {
        screen
            .banner
            .as_ref()
            .is_some_and(|banner| banner.text == "Process exited with code 3")
    });
    press(&mut editor, "RET");
    wait_for(&mut editor, "a new shell", |screen| {
        screen.banner.is_none() && screen.notice.is_none()
    });
    type_text(&mut editor, "exit\n");
    wait_for(&mut editor, "the exit", |screen| screen.banner.is_some());
    press(&mut editor, "q");
    assert_eq!(editor.shown_pane(), None);
    assert!(
        editor
            .buffers()
            .iter()
            .all(|buffer| !buffer.name().starts_with("*term"))
    );
}

#[test]
fn a_shell_that_is_not_found_says_so_and_another_can_start() {
    let mut editor = editor("shell = \"no-such-shell-anywhere\"\n");
    press(&mut editor, "C-`");
    let screen = shown(&editor).unwrap();
    assert_eq!(
        screen.notice.map(|notice| notice.title),
        Some("Shell not found".to_owned())
    );
    press(&mut editor, "z");
    wait_for(&mut editor, "zsh", |screen| screen.notice.is_none());
}

#[test]
fn commands_the_shell_reports_are_marked_by_their_prompt() {
    let script = r"printf '\033]133;A\007P1> \033]133;C\007\nout\n\033]133;D;7\007\033]133;A\007P2> '; sleep 5";
    let mut editor = editor(&format!(
        "shell = \"/bin/sh\"\nargs = [\"-c\", {script:?}]\n"
    ));
    press(&mut editor, "C-`");
    wait_for_text(&mut editor, "P2>");
    let screen = shown(&editor).unwrap();
    let marked: Vec<_> = screen
        .rows
        .iter()
        .filter_map(|row| row.mark.as_ref().map(|mark| mark.exit))
        .collect();
    assert_eq!(marked, vec![Some(7)]);
}

#[test]
fn the_marks_hide_while_a_program_draws_its_own_screen() {
    // A finished command is marked; the next runs a program that takes the keys as
    // Claude Code does, drawing in place.
    let script = r"printf '\033]133;A\007P1> \033]133;C\007\nout\n\033]133;D;0\007\033]133;A\007P2> \033]133;C\007'; sleep 1; printf '\033[?2004h\033[>5utui'; sleep 1; printf '\033[?2004l\033[<u\033[2J\033[Hclear'; sleep 5";
    let mut editor = editor(&format!(
        "shell = \"/bin/sh\"\nargs = [\"-c\", {script:?}]\n"
    ));
    press(&mut editor, "C-`");
    let marked = |screen: &Screen| screen.rows.iter().any(|row| row.mark.is_some());
    wait_for(&mut editor, "the mark", |screen| {
        text(screen).contains("P2>") && marked(screen)
    });
    wait_for(&mut editor, "the program", |screen| {
        text(screen).contains("tui") && !marked(screen)
    });
    // Left the keys, but cleared the screen: no mark shows.
    wait_for(&mut editor, "no mark", |screen| {
        text(screen).contains("clear") && !marked(screen)
    });
}

/// A terminal in the panel running `script` in sh, its buffer's name.
fn running(script: &str) -> (Editor, String) {
    let mut editor = editor(&format!(
        "shell = \"/bin/sh\"\nargs = [\"-c\", {script:?}]\n"
    ));
    press(&mut editor, "C-`");
    let name = terminals::buffer_name(&editor, in_panel(&editor).unwrap()).unwrap();
    (editor, name)
}

fn pointer(row: usize, column: usize, modifiers: &str, mouse: ScreenMouse) -> ScreenPointer {
    let key = Key::parse(&format!("{modifiers}a")).unwrap();
    ScreenPointer {
        row,
        column,
        modifiers: key.modifiers,
        mouse,
    }
}

#[test]
fn command_click_visits_a_file_line_in_the_output() {
    let path = format!("/tmp/remacs-link-{}.rs", std::process::id());
    std::fs::write(&path, "one\ntwo three\n").unwrap();
    let (mut editor, name) = running(&format!("printf 'see {path}:2:5 now'; sleep 5"));
    wait_for_text(&mut editor, "now");
    let screen = shown(&editor).unwrap();
    let column = text(&screen).find(&path).unwrap() + 3;
    assert!(!editor.screen_mouse(&name, pointer(0, column, "", ScreenMouse::Move)));
    assert!(editor.screen_mouse(&name, pointer(0, column, "s-", ScreenMouse::Move)));
    let screen = shown(&editor).unwrap();
    assert!(screen.pointing);
    let press = ScreenMouse::Press {
        button: ScreenButton::Left,
        clicks: 1,
    };
    editor.screen_mouse(&name, pointer(0, column, "s-", press));
    std::fs::remove_file(&path).unwrap();
    let buffer = editor.window_buffer();
    assert!(buffer.name().starts_with("remacs-link-"));
    assert_eq!(
        buffer.contents(buffer.point()..buffer.text().len()),
        "three\n"
    );
}

#[test]
fn programs_that_ask_get_the_mouse_and_the_keys_they_tell_apart() {
    let script = r"stty raw -echo; printf '\033[?1000h\033[?1006h\033[>1u\033[?uready'; dd bs=1 count=22 2>/dev/null | od -An -c | tr -s ' \n' ' '; sleep 5";
    let (mut editor, name) = running(script);
    wait_for_text(&mut editor, "ready");
    let wheel = ScreenMouse::Wheel { rows: 1 };
    editor.screen_mouse(&name, pointer(2, 4, "", wheel));
    press(&mut editor, "S-RET");
    // The answer to the query, the wheel at row 3 column 5, then ⇧↵; on one line, as raw
    // mode returns no carriage at od's line ends.
    wait_for_text(&mut editor, "? 1 u");
    wait_for_text(&mut editor, "< 6 5 ; 5 ; 3 M");
    wait_for_text(&mut editor, "1 3 ; 2 u");
}

/// `shell` started with its hooks in a home of its own holding `files`; checks that it
/// read them, marks commands by how they ended, and reports its directory.
fn reports_with_hooks(shell: &str, args: &str, files: &[(&str, &str)], prompt: &str) {
    let home = env::temp_dir().join(format!("remacs-hooks-{shell}-{}", std::process::id()));
    fs::create_dir_all(&home).unwrap();
    for (name, contents) in files {
        fs::write(home.join(name), contents).unwrap();
    }
    let mut editor = editor(&format!(
        "shell = \"{shell}\"\nargs = {args}\n[env]\nHOME = {home:?}\nTERM = \"xterm-256color\"\n"
    ));
    press(&mut editor, "C-`");
    wait_for_text(&mut editor, prompt);
    type_text(&mut editor, "false\n");
    wait_for(&mut editor, "the mark", |screen| {
        screen
            .rows
            .iter()
            .any(|row| row.mark.as_ref().is_some_and(|mark| mark.exit == Some(1)))
    });
    type_text(
        &mut editor,
        "cd /usr; echo \"[$REMACS_BASH_INJECT$REMACS_BASH_LOGIN$REMACS_ZSH_ZDOTDIR]\"\n",
    );
    wait_for_text(&mut editor, "\n[]");
    let id = in_panel(&editor).unwrap();
    let start = Instant::now();
    while terminals::find(&editor, id).unwrap().directory != Path::new("/usr") {
        assert!(start.elapsed() < WAIT, "the directory was not reported");
        editor.refresh();
        thread::sleep(Duration::from_millis(10));
    }
    fs::remove_dir_all(&home).unwrap();
}

#[test]
fn zsh_and_bash_report_their_commands_through_injected_hooks() {
    reports_with_hooks("/bin/zsh", "[]", &[(".zshrc", "PS1='zrc> '\n")], "zrc>");
    reports_with_hooks(
        "/bin/bash",
        "[\"--login\"]",
        &[(".bash_profile", "PS1='bprofile> '\n")],
        "bprofile>",
    );
}

#[test]
fn copy_mode_searches_the_scrollback_and_copies() {
    let mut editor = editor(SH);
    press(&mut editor, "C-`");
    wait_for(&mut editor, "the prompt", |screen| screen.notice.is_none());
    type_text(&mut editor, "echo needle; echo hay; echo needle\n");
    // Twice in the command line, twice in its output.
    wait_for(&mut editor, "the output", |screen| {
        text(screen).matches("needle").count() == 4
    });
    press(&mut editor, "C-x [");
    let screen = shown(&editor).unwrap();
    assert_eq!(screen.mode.map(|mode| mode.label), Some("COPY"));
    press(&mut editor, "C-s");
    type_text(&mut editor, "needle");
    let found = shown(&editor)
        .unwrap()
        .search
        .and_then(|search| search.found);
    assert_eq!(found.map(|(_, total)| total), Some(4));
    press(&mut editor, "RET C-SPC M-f M-w");
    assert_eq!(editor.kill_ring.newest(), Some("needle"));
    assert!(
        shown(&editor).unwrap().mode.is_none(),
        "copying leaves copy mode"
    );
}
