use remacs_core::{Buffer, Code, Documentation, Editor, Key, KeyCode, Modifiers, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_text::Text;

use crate::{CompletionPlugin, Item, List, Request, Trigger, deliver, resolved, take_requests};

fn editor_with(text: &str) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &CompletionPlugin]);
    editor.add_buffer(Buffer::new("buffer.rs", Text::from(text), "Rust"));
    let end = editor.window_buffer().text().len();
    editor.window_buffer_mut().set_point(end);
    editor
}

fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        editor.handle_key(Key::new(Modifiers::default(), KeyCode::Char(character)));
    }
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn item(label: &str, start: usize) -> Item {
    Item {
        label: label.into(),
        filter_text: label.trim_end_matches("()").into(),
        insert: label.into(),
        start,
        ..Item::default()
    }
}

fn list(labels: &[&str], start: usize) -> List {
    List { items: labels.iter().map(|label| item(label, start)).collect(), incomplete: false }
}

fn shown(editor: &Editor) -> Vec<String> {
    editor.popups().into_iter().flat_map(|popup| popup.rows).map(|row| row.label).collect()
}

fn text(editor: &Editor) -> String {
    let buffer = editor.window_buffer();
    buffer.contents(0..buffer.text().len())
}

fn complete_requests(editor: &mut Editor) -> Vec<Request> {
    take_requests(editor)
        .into_iter()
        .filter(|request| matches!(request, Request::Complete { .. }))
        .collect()
}

#[test]
fn typing_asks_once_per_word_and_narrows_the_list() {
    let mut editor = editor_with("gap.");
    type_text(&mut editor, "s");
    let request = Request::Complete { id: 1, start: 4, offset: 5, trigger: Trigger::Typed('s') };
    assert_eq!(take_requests(&mut editor), [request]);
    type_text(&mut editor, "t");
    assert_eq!(take_requests(&mut editor), []);

    let labels = ["size_hint()", "last()", "start", "other", "step_by()", "is_sorted()"];
    deliver(&mut editor, 1, list(&labels, 4));
    assert_eq!(shown(&editor), ["start", "step_by()", "size_hint()", "last()", "is_sorted()"]);
    let popup = editor.popups().remove(0);
    assert_eq!((popup.anchor, popup.selected), (4, Some(0)));
    assert_eq!(popup.rows[2].matches, [0..1, 8..9]);
    assert_eq!(take_requests(&mut editor), [Request::Resolve { id: 1, index: 2 }]);

    type_text(&mut editor, "a");
    assert_eq!(shown(&editor), ["start"]);
    press(&mut editor, "DEL DEL");
    assert_eq!(shown(&editor), ["size_hint()", "start", "step_by()", "last()", "is_sorted()"]);
    press(&mut editor, "C-n C-n RET");
    assert_eq!(text(&editor), "gap.step_by()");
    assert!(editor.popups().is_empty());
}

#[test]
fn items_starting_before_the_word_match_the_text_from_their_start() {
    let mut editor = editor_with("x bg-");
    type_text(&mut editor, "r");
    let classes = ["ring-red-500", "bg-red-500", "border-red-500"].map(|label| item(label, 2));
    let mut items = vec![item("resize", 5)];
    items.extend(classes);
    deliver(&mut editor, 1, List { items, incomplete: false });
    assert_eq!(shown(&editor), ["resize", "bg-red-500"]);
    assert_eq!(editor.popups()[0].rows[1].matches.first(), Some(&(0..4)));
    press(&mut editor, "C-n RET");
    assert_eq!(text(&editor), "x bg-red-500");
}

#[test]
fn other_commands_and_non_word_characters_close_the_popup() {
    let mut editor = editor_with("x\ngap");
    press(&mut editor, "C-M-i");
    assert_eq!(
        take_requests(&mut editor),
        [Request::Complete { id: 1, start: 2, offset: 5, trigger: Trigger::Explicit }]
    );
    deliver(&mut editor, 1, list(&["gap", "gape"], 2));
    assert_eq!(shown(&editor).len(), 2);
    type_text(&mut editor, ".");
    assert!(shown(&editor).is_empty());
    assert_eq!(
        complete_requests(&mut editor),
        [Request::Complete { id: 2, start: 6, offset: 6, trigger: Trigger::Typed('.') }]
    );

    deliver(&mut editor, 2, list(&["len()"], 6));
    assert_eq!(shown(&editor), ["len()"]);
    press(&mut editor, "C-a");
    assert!(shown(&editor).is_empty());
    press(&mut editor, "C-p");
    assert_eq!(editor.window_buffer().point_line(), 0);
}

#[test]
fn incomplete_lists_are_asked_for_again_and_stale_ones_dropped() {
    let mut editor = editor_with("");
    type_text(&mut editor, "pr");
    take_requests(&mut editor);
    let mut first = list(&["print", "proc"], 0);
    first.incomplete = true;
    deliver(&mut editor, 1, first);
    assert_eq!(
        complete_requests(&mut editor),
        [Request::Complete { id: 2, start: 0, offset: 2, trigger: Trigger::Incomplete }]
    );
    deliver(&mut editor, 1, list(&["stale"], 0));
    assert_eq!(shown(&editor), ["print", "proc"]);

    type_text(&mut editor, "i");
    assert_eq!(shown(&editor), ["print"]);
    type_text(&mut editor, "n");
    assert_eq!(complete_requests(&mut editor).len(), 2);
    deliver(&mut editor, 3, list(&["superseded"], 0));
    assert_eq!(shown(&editor), ["print"]);
    deliver(&mut editor, 4, list(&["println", "printf"], 0));
    assert_eq!(shown(&editor), ["println", "printf"]);
}

#[test]
fn inserting_places_point_and_makes_extra_edits() {
    let mut editor = editor_with("use a;\nfn f() { ma");
    press(&mut editor, "C-M-i");
    take_requests(&mut editor);
    let start = "use a;\nfn f() { ".len();
    let documentation = Documentation {
        kind: "macro".into(),
        signature: Some(Code::new("macro_rules! map", Some("Rust"))),
        ..Documentation::default()
    };
    let map = Item {
        label: "map!".into(),
        filter_text: "map".into(),
        insert: "map!()".into(),
        cursor: Some(4),
        start,
        extra_edits: vec![(0..0, "use m::map;\n".into())],
        ..Item::default()
    };
    deliver(&mut editor, 1, List { items: vec![map.clone()], incomplete: false });
    assert_eq!(editor.popups()[0].documentation, None);
    resolved(&mut editor, 1, 0, Item { documentation: Some(documentation.clone()), ..map });
    assert_eq!(editor.popups()[0].documentation, Some(documentation));
    press(&mut editor, "TAB");
    assert_eq!(text(&editor), "use m::map;\nuse a;\nfn f() { map!()");
    let point = editor.window_buffer().point();
    assert_eq!(editor.window_buffer().contents(point - 4..point), "map!");
    press(&mut editor, "C-/");
    assert_eq!(text(&editor), "use a;\nfn f() { ma");
}

/// Snippets are written at column 0 with tabs, as servers send them for any editor.
#[test]
fn inserted_lines_take_the_lines_level_and_the_buffers_unit() {
    let mut editor = editor_with("fn f() {\n  g();\n  if");
    press(&mut editor, "C-M-i");
    take_requests(&mut editor);
    let start = "fn f() {\n  g();\n  ".len();
    let snippet = Item { insert: "if b {\n\t\n}".into(), cursor: Some(8), ..item("if", start) };
    deliver(&mut editor, 1, List { items: vec![snippet], incomplete: false });
    press(&mut editor, "TAB");
    assert_eq!(text(&editor), "fn f() {\n  g();\n  if b {\n    \n  }");
    assert_eq!(editor.window_buffer().point(), "fn f() {\n  g();\n  if b {\n    ".len());
}

#[test]
fn moving_point_without_a_command_hides_the_popup() {
    let mut editor = editor_with("abc");
    press(&mut editor, "M-TAB");
    take_requests(&mut editor);
    deliver(&mut editor, 1, list(&["abcd"], 0));
    editor.refresh();
    assert_eq!(shown(&editor), ["abcd"]);
    editor.set_point_at(0, 0);
    assert!(shown(&editor).is_empty());
    press(&mut editor, "C-n");
    editor.set_point_at(0, 3);
    assert!(shown(&editor).is_empty(), "C-n ended the session");
}

#[test]
fn an_empty_answer_to_an_explicit_request_is_reported() {
    let mut editor = editor_with("zz");
    press(&mut editor, "C-M-i");
    deliver(&mut editor, 1, list(&[], 0));
    assert_eq!(editor.echo().as_deref(), Some("No completions"));
}
