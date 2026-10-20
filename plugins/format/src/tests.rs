use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use remacs_core::{Buffer, Editor, Embedded, Plugin, Registry, Settings, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_text::Text;

use crate::{Failure, FormatPlugin, Request, deliver, edits, take_requests};

const UGLY: &str = "fn main(){\n    call(a,b);\n}\n";
const PRETTY: &str = "fn main() {\n    call(a, b);\n}\n";

#[test]
fn the_default_file_reads_as_the_defaults() {
    remacs_core::assert_default_file::<crate::FormatSettings>();
}

fn editor() -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &FormatPlugin]);
    let path = PathBuf::from("/project/main.rs");
    editor.add_buffer(Buffer::new("main.rs", Text::from(UGLY), "Rust").with_file(path));
    editor
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn contents(editor: &Editor) -> String {
    let buffer = editor.window_buffer();
    buffer.contents(0..buffer.text().len())
}

#[test]
fn formats_the_buffer_keeping_point_on_its_character_in_one_undo_step() {
    let mut editor = editor();
    let b = UGLY.find('b').unwrap();
    editor.window_buffer_mut().set_point(b);
    press(&mut editor, "C-c C-f");
    let requests = take_requests(&mut editor, |_| true);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].range, None);
    assert_eq!(requests[0].mode, "Rust");
    deliver(&mut editor, requests[0].id, Ok(edits(UGLY, PRETTY)));
    assert_eq!(contents(&editor), PRETTY);
    assert_eq!(editor.window_buffer().point(), PRETTY.find('b').unwrap());
    press(&mut editor, "C-/");
    assert_eq!(contents(&editor), UGLY);
}

#[test]
fn the_region_is_formatted_when_active() {
    let mut editor = editor();
    let buffer = editor.window_buffer_mut();
    buffer.set_mark(4);
    buffer.activate_mark();
    buffer.set_point(9);
    press(&mut editor, "C-c C-f");
    let requests = take_requests(&mut editor, |_| true);
    assert_eq!(requests[0].range, Some(4..9));
}

#[test]
fn backends_take_only_what_they_accept() {
    let mut editor = editor();
    press(&mut editor, "C-c C-f");
    assert!(take_requests(&mut editor, |request| request.mode == "CSS").is_empty());
    assert_eq!(take_requests(&mut editor, |_| true).len(), 1);
}

#[test]
fn edits_for_a_changed_buffer_and_failures_are_not_applied() {
    let mut editor = editor();
    press(&mut editor, "C-c C-f");
    let request = take_requests(&mut editor, |_| true).remove(0);
    editor.window_buffer_mut().insert("// ");
    deliver(&mut editor, request.id, Ok(edits(UGLY, PRETTY)));
    assert_eq!(contents(&editor), format!("// {UGLY}"));
    assert_eq!(
        editor.echo().as_deref(),
        Some("Not formatted: the buffer changed meanwhile")
    );

    press(&mut editor, "C-c C-f");
    let request = take_requests(&mut editor, |_| true).remove(0);
    let failed = Failure::Failed("prettier: Unexpected token".into());
    deliver(&mut editor, request.id, Err(failed));
    assert_eq!(editor.echo().as_deref(), Some("prettier: Unexpected token"));
}

const PAGE: &str = "<p>x</p>\n<style>a{color:red}</style>\n";

/// Tells the code in `PAGE`'s `<style>`.
struct StylePlugin;

impl Plugin for StylePlugin {
    fn register(&self, registry: &mut Registry) {
        let style = PAGE.find("a{").unwrap()..PAGE.find("</style>").unwrap();
        registry.embedded_probes.push(Rc::new(move |_| {
            Some(vec![Embedded {
                mode: "CSS",
                ranges: vec![style.clone()],
            }])
        }));
    }
}

#[test]
fn a_region_of_embedded_code_is_its_language_s_and_its_edits_stay_in_it() {
    let mut editor = Editor::new(&[&EditingPlugin, &FormatPlugin, &StylePlugin]);
    let path = PathBuf::from("/project/page.html");
    editor.add_buffer(Buffer::new("page.html", Text::from(PAGE), "HTML").with_file(path));
    let (start, end) = (PAGE.find("a{").unwrap(), PAGE.find("</style>").unwrap());
    let select_style = |editor: &mut Editor| {
        let buffer = editor.window_buffer_mut();
        buffer.set_mark(start);
        buffer.activate_mark();
        buffer.set_point(end);
        press(editor, "C-c C-f");
        take_requests(editor, |_| true).remove(0)
    };

    let request = select_style(&mut editor);
    assert_eq!(request.mode, "CSS");
    assert_eq!(
        request.path,
        PathBuf::from("/project/page.html.remacs-embedded.CSS")
    );
    let text = request.text.bytes_in(0..request.text.len());
    assert_eq!(String::from_utf8(text).unwrap().trim(), "a{color:red}");
    deliver(
        &mut editor,
        request.id,
        Ok(vec![(start + 1..start + 1, " ".into())]),
    );
    assert!(contents(&editor).contains("<style>a {color:red}</style>"));

    let request = select_style(&mut editor);
    deliver(&mut editor, request.id, Ok(vec![(0..3, String::new())]));
    assert!(contents(&editor).starts_with("<p>x</p>"));
    assert_eq!(
        editor.echo().as_deref(),
        Some("Not formatted: the edits reach outside the embedded code")
    );
}

/// An editor visiting a file holding `UGLY`, formatting on save turned on.
fn formatting_on_save(name: &str) -> (Editor, PathBuf) {
    let path = std::env::temp_dir().join(format!("remacs-format-{}-{name}.rs", std::process::id()));
    fs::write(&path, UGLY).unwrap();
    let mut editor = Editor::new(&[&EditingPlugin, &FormatPlugin]);
    editor.visit_file(&path).unwrap();
    editor.window_buffer_mut().save().unwrap();
    editor.refresh();
    assert!(
        take_requests(&mut editor, |_| true).is_empty(),
        "off by default"
    );
    editor.run_command("format-on-save-mode");
    assert_eq!(editor.echo().as_deref(), Some("Formatting on save enabled"));
    (editor, path)
}

fn save(editor: &mut Editor) -> Request {
    editor.window_buffer_mut().save().unwrap();
    editor.refresh();
    let mut requests = take_requests(editor, |_| true);
    assert_eq!(requests.len(), 1);
    requests.remove(0)
}

#[test]
fn saved_files_are_formatted_and_saved_again() {
    let (mut editor, path) = formatting_on_save("again");
    let request = save(&mut editor);
    deliver(&mut editor, request.id, Ok(edits(UGLY, PRETTY)));
    assert_eq!(fs::read_to_string(&path).unwrap(), PRETTY);
    assert!(!editor.window_buffer().is_modified());
    editor.refresh();
    assert!(
        take_requests(&mut editor, |_| true).is_empty(),
        "its own save is not formatted again"
    );
    press(&mut editor, "C-/");
    editor.refresh();
    assert!(take_requests(&mut editor, |_| true).is_empty());
    fs::remove_file(path).unwrap();
}

#[test]
fn after_a_save_typing_and_files_nothing_formats_are_left_alone() {
    let (mut editor, path) = formatting_on_save("alone");
    let request = save(&mut editor);
    editor.window_buffer_mut().insert("// ");
    deliver(&mut editor, request.id, Ok(edits(UGLY, PRETTY)));
    assert_eq!(contents(&editor), format!("// {UGLY}"));
    let request = save(&mut editor);
    let unsupported = Failure::Unsupported("No formatter for this buffer".into());
    deliver(&mut editor, request.id, Err(unsupported));
    assert_eq!(editor.echo().as_deref(), Some("Formatting on save enabled"));
    assert_eq!(fs::read_to_string(&path).unwrap(), format!("// {UGLY}"));
    fs::remove_file(path).unwrap();
}

fn settings(files: &[(&str, &str)]) -> Settings {
    Settings::from_files(
        files
            .iter()
            .map(|(name, text)| (PathBuf::from(name), text.to_string())),
    )
}

fn formats_on_save(editor: &mut Editor) -> bool {
    editor.window_buffer_mut().save().unwrap();
    editor.refresh();
    !take_requests(editor, |_| true).is_empty()
}

#[test]
fn the_settings_turn_formatting_on_save_on_and_a_toggle_lasts_until_they_change() {
    let path =
        std::env::temp_dir().join(format!("remacs-format-{}-settings.rs", std::process::id()));
    fs::write(&path, UGLY).unwrap();
    let mut editor = Editor::new(&[&EditingPlugin, &FormatPlugin]);
    editor.set_settings(settings(&[("format.toml", "on-save = true\n")]));
    editor.visit_file(&path).unwrap();
    assert!(formats_on_save(&mut editor));

    editor.run_command("format-on-save-mode");
    editor.set_settings(settings(&[
        ("format.toml", "on-save = true\n"),
        ("ui.toml", ""),
    ]));
    assert!(!formats_on_save(&mut editor), "other files changed");

    editor.set_settings(settings(&[]));
    assert!(!formats_on_save(&mut editor));
    editor.set_settings(settings(&[("format.toml", "on-save = true\n")]));
    assert!(formats_on_save(&mut editor));
    fs::remove_file(path).unwrap();
}

#[test]
fn a_wrong_setting_is_shown_and_leaves_formatting_on_save_off() {
    let mut editor = editor();
    editor.set_settings(settings(&[("format.toml", "on-save = \"yes\"\n")]));
    let message = editor.echo().unwrap_or_default();
    assert!(message.starts_with("format.toml:1: "), "{message}");
    assert!(!editor.plugin_state::<crate::State>().on_save);
}
