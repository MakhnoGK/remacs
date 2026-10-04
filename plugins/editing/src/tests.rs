use remacs_core::{Buffer, Editor, Key, Mode, Plugin, Registry, parse_sequence};
use remacs_text::Text;

use crate::EditingPlugin;

const CODE_MODE: &str = "Code";

struct CodeModePlugin;

impl Plugin for CodeModePlugin {
    fn register(&self, registry: &mut Registry) {
        registry.modes.add(Mode {
            name: CODE_MODE,
            extensions: &[],
            pairs: &[('(', ')'), ('[', ']'), ('{', '}')],
        });
    }
}

fn editor_in_mode(text: &str, mode: &'static str) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &CodeModePlugin]);
    editor.add_buffer(Buffer::new("test", Text::from(text), mode));
    editor.viewport.set_height(10);
    editor
}

fn editor_with(text: &str) -> Editor {
    editor_in_mode(text, "Fundamental")
}

fn code_editor_with(text: &str) -> Editor {
    editor_in_mode(text, CODE_MODE)
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        editor.handle_key(Key::parse(&character.to_string()).unwrap());
    }
}

fn contents(editor: &Editor) -> String {
    let buffer = editor.buffer();
    buffer.contents(0..buffer.text().len())
}

#[test]
fn typing_inserts_at_point() {
    let mut editor = editor_with("world");
    type_text(&mut editor, "hello ");
    assert_eq!(contents(&editor), "hello world");
    assert_eq!(editor.buffer().point(), 6);
}

#[test]
fn vertical_motion_keeps_goal_column() {
    let mut editor = editor_with("abcdef\nab\nabcdef");
    press(&mut editor, "C-e C-n");
    assert_eq!(editor.buffer().point(), 9);
    press(&mut editor, "C-n");
    assert_eq!(editor.buffer().point(), 16);
    press(&mut editor, "C-n");
    assert_eq!(editor.buffer().point(), 16);
    assert_eq!(editor.echo().as_deref(), Some("End of buffer"));
}

#[test]
fn motion_steps_over_multibyte_characters() {
    let mut editor = editor_with("жёлтый");
    press(&mut editor, "C-f C-f");
    assert_eq!(editor.buffer().point(), 4);
    press(&mut editor, "DEL");
    assert_eq!(contents(&editor), "жлтый");
}

#[test]
fn word_motion_skips_punctuation() {
    let mut editor = editor_with("fn  main() -> жук");
    press(&mut editor, "M-f M-f");
    assert_eq!(editor.buffer().point(), 8);
    press(&mut editor, "M-f");
    assert_eq!(editor.buffer().point(), "fn  main() -> жук".len());
    press(&mut editor, "M-b M-b");
    assert_eq!(editor.buffer().point(), 4);
}

#[test]
fn newline_keeps_indentation() {
    let mut editor = editor_with("    let x = 1;");
    press(&mut editor, "C-e RET");
    type_text(&mut editor, "y");
    assert_eq!(contents(&editor), "    let x = 1;\n    y");
}

#[test]
fn newline_after_an_open_bracket_goes_a_level_deeper() {
    let mut editor = editor_with("  if x {\n    y();\n  }");
    press(&mut editor, "C-n C-e RET");
    type_text(&mut editor, "z(1,");
    press(&mut editor, "RET");
    type_text(&mut editor, "2");
    assert_eq!(
        contents(&editor),
        "  if x {\n    y();\n    z(1,\n      2\n  }"
    );
}

#[test]
fn newline_between_an_empty_pair_gives_the_closer_its_own_line() {
    let mut editor = editor_with("\tfn f() {}");
    press(&mut editor, "C-e C-b RET");
    type_text(&mut editor, "x");
    assert_eq!(contents(&editor), "\tfn f() {\n\t\tx\n\t}");
}

#[test]
fn newline_before_a_closer_indents_to_its_opener() {
    let mut editor = editor_with("fn f() {\n    g()}");
    press(&mut editor, "M-> C-b RET");
    assert_eq!(contents(&editor), "fn f() {\n    g()\n}");
    assert_eq!(editor.buffer().point(), "fn f() {\n    g()\n".len());
}

#[test]
fn newline_leaves_no_trailing_blanks() {
    let mut editor = editor_with("    a   b");
    press(&mut editor, "C-e M-b RET");
    assert_eq!(contents(&editor), "    a\n    b");
    press(&mut editor, "C-e RET RET");
    assert_eq!(contents(&editor), "    a\n    b\n\n    ");
}

#[test]
fn closer_typed_on_a_blank_line_dedents_to_its_opener() {
    let mut editor = editor_with("fn f() {\n    g(\n        1,\n        ");
    press(&mut editor, "M->");
    type_text(&mut editor, ")");
    press(&mut editor, "RET");
    type_text(&mut editor, "}");
    assert_eq!(contents(&editor), "fn f() {\n    g(\n        1,\n    )\n}");
}

#[test]
fn lines_open_below_and_above_without_breaking_this_one() {
    let mut editor = editor_with("fn f() {\n    g();\n}");
    press(&mut editor, "C-f s-RET");
    type_text(&mut editor, "a");
    assert_eq!(contents(&editor), "fn f() {\n    a\n    g();\n}");
    press(&mut editor, "M-> s-S-RET");
    type_text(&mut editor, "b");
    assert_eq!(contents(&editor), "fn f() {\n    a\n    g();\n    b\n}");
    press(&mut editor, "C-p C-e s-S-RET");
    type_text(&mut editor, "c");
    assert_eq!(
        contents(&editor),
        "fn f() {\n    a\n    c\n    g();\n    b\n}"
    );
}

#[test]
fn brackets_close_themselves_and_are_typed_over() {
    let mut editor = code_editor_with("");
    type_text(&mut editor, "f(a[1");
    assert_eq!(contents(&editor), "f(a[1])");
    type_text(&mut editor, "]);");
    assert_eq!(contents(&editor), "f(a[1]);");
    assert_eq!(editor.buffer().point(), "f(a[1]);".len());
}

#[test]
fn an_opener_before_a_word_stays_single() {
    let mut editor = code_editor_with("x");
    type_text(&mut editor, "(");
    assert_eq!(contents(&editor), "(x");
}

#[test]
fn a_closer_is_typed_when_the_line_needs_one() {
    let mut editor = code_editor_with("f(g(x)");
    press(&mut editor, "C-e C-b");
    type_text(&mut editor, ")");
    assert_eq!(contents(&editor), "f(g(x))");
}

#[test]
fn delete_takes_an_empty_pair_whole() {
    let mut editor = code_editor_with("");
    type_text(&mut editor, "{");
    press(&mut editor, "DEL");
    assert_eq!(contents(&editor), "");
}

#[test]
fn an_opener_wraps_the_region_and_keeps_it() {
    let mut editor = code_editor_with("let x = a + b;");
    press(&mut editor, "M-f M-f M-f M-b C-SPC C-e C-b");
    type_text(&mut editor, "(");
    assert_eq!(contents(&editor), "let x = (a + b);");
    type_text(&mut editor, "[");
    assert_eq!(contents(&editor), "let x = ([a + b]);");
    assert_eq!(editor.buffer().region(), Some(10..15));
}

#[test]
fn newline_inside_a_typed_pair_opens_a_block() {
    let mut editor = code_editor_with("fn f() ");
    press(&mut editor, "C-e");
    type_text(&mut editor, "{");
    press(&mut editor, "RET");
    type_text(&mut editor, "x");
    assert_eq!(contents(&editor), "fn f() {\n    x\n}");
}

#[test]
fn fundamental_mode_pairs_nothing() {
    let mut editor = editor_with("");
    type_text(&mut editor, "(");
    assert_eq!(contents(&editor), "(");
}

#[test]
fn tab_follows_the_buffers_indentation() {
    let mut editor = editor_with("a {\n  b\n}\n");
    press(&mut editor, "M-> TAB");
    type_text(&mut editor, "c");
    press(&mut editor, "TAB");
    assert_eq!(contents(&editor), "a {\n  b\n}\n  c ");
}

#[test]
fn consecutive_typing_undoes_as_one_step() {
    let mut editor = editor_with("");
    type_text(&mut editor, "abc");
    press(&mut editor, "RET");
    type_text(&mut editor, "de");
    press(&mut editor, "C-/");
    assert_eq!(contents(&editor), "abc\n");
    press(&mut editor, "C-/ C-/");
    assert_eq!(contents(&editor), "");
    press(&mut editor, "C-?");
    assert_eq!(contents(&editor), "abc");
}

#[test]
fn prefix_keys_wait_and_abort() {
    let mut editor = editor_with("text");
    press(&mut editor, "C-x");
    assert_eq!(editor.echo().as_deref(), Some("C-x-"));
    press(&mut editor, "C-g");
    assert_eq!(editor.echo().as_deref(), Some("Quit"));
    press(&mut editor, "C-x C-q");
    assert_eq!(editor.echo().as_deref(), Some("C-x C-q is undefined"));
}

#[test]
fn paging_moves_point_into_view() {
    let lines: String = (0..100).map(|line| format!("{line}\n")).collect();
    let mut editor = editor_with(&lines);
    press(&mut editor, "C-v");
    assert_eq!(editor.viewport.first_line, 8);
    assert_eq!(editor.buffer().point_line(), 8);
    press(&mut editor, "M-> M-v");
    assert!(editor.viewport.contains(editor.buffer().point_line()));
}

#[test]
fn region_is_killed_and_yanked_back() {
    let mut editor = editor_with("hello brave world");
    press(&mut editor, "M-f C-SPC M-f");
    assert_eq!(editor.buffer().region(), Some(5..11));
    press(&mut editor, "C-w");
    assert_eq!(contents(&editor), "hello world");
    assert!(editor.buffer().region().is_none());
    press(&mut editor, "C-e C-y");
    assert_eq!(contents(&editor), "hello world brave");
    press(&mut editor, "C-x h M-w");
    assert_eq!(editor.kill_ring.newest(), Some("hello world brave"));
}

#[test]
fn consecutive_kills_yank_as_one() {
    let mut editor = editor_with("one\ntwo\nthree");
    press(&mut editor, "C-k C-k");
    assert_eq!(contents(&editor), "two\nthree");
    press(&mut editor, "C-y");
    assert_eq!(contents(&editor), "one\ntwo\nthree");
    press(&mut editor, "M-> M-DEL M-DEL");
    assert_eq!(editor.kill_ring.newest(), Some("two\nthree"));
}

#[test]
fn yank_pop_cycles_through_earlier_kills() {
    let mut editor = editor_with("alpha beta ");
    press(&mut editor, "M-d C-f M-d C-e C-y");
    assert_eq!(contents(&editor), "  beta");
    press(&mut editor, "M-y");
    assert_eq!(contents(&editor), "  alpha");
    press(&mut editor, "C-a M-y");
    assert!(editor.minibuffer().is_some());
    editor.exit_minibuffer(false).unwrap();
    assert_eq!(contents(&editor), "beta  alpha");
}

#[test]
fn delete_removes_an_active_region_without_killing_it() {
    let mut editor = editor_with("keep drop");
    press(&mut editor, "M-f C-SPC C-e DEL");
    assert_eq!(contents(&editor), "keep");
    assert!(editor.kill_ring.is_empty());
}
