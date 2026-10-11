use std::rc::Rc;

use remacs_core::{
    Buffer, CommentSyntax, Editor, Key, Mode, Plugin, Registry, SyntaxContext, parse_sequence,
};
use remacs_text::Text;

use crate::EditingPlugin;

const CODE_MODE: &str = "Code";
const STYLE_MODE: &str = "Style";

struct CodeModePlugin;

impl Plugin for CodeModePlugin {
    fn register(&self, registry: &mut Registry) {
        registry.modes.add(Mode {
            name: CODE_MODE,
            extensions: &[],
            file_names: &[],
            pairs: &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')],
            comment: Some(CommentSyntax {
                start: "// ",
                end: "",
            }),
            markup_comment: Some(CommentSyntax {
                start: "{/* ",
                end: " */}",
            }),
        });
        registry.modes.add(Mode {
            name: STYLE_MODE,
            extensions: &[],
            file_names: &[],
            pairs: &[],
            comment: Some(CommentSyntax {
                start: "/* ",
                end: " */",
            }),
            markup_comment: None,
        });
        registry.syntax_probes.push(Rc::new(in_double_quotes));
    }
}

/// Stands in for a parser: an offset after an odd number of `"` on its line is in a string,
/// one right after `<` is in markup.
fn in_double_quotes(buffer: &Buffer, offset: usize) -> Option<SyntaxContext> {
    let line_start = buffer.text().line_start(buffer.text().line_of(offset))?;
    let before = buffer.contents(line_start..offset);
    Some(if before.matches('"').count() % 2 == 1 {
        SyntaxContext::String
    } else if before.ends_with('<') {
        SyntaxContext::Markup
    } else {
        SyntaxContext::Code
    })
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
fn word_motion_stops_between_letters_and_other_characters() {
    let mut editor = editor_with("fn  main() -> жук");
    press(&mut editor, "M-f M-f");
    assert_eq!(editor.buffer().point(), 4);
    press(&mut editor, "M-f M-f");
    assert_eq!(editor.buffer().point(), "fn  main() -> ".len());
    press(&mut editor, "M-f");
    assert_eq!(editor.buffer().point(), "fn  main() -> жук".len());
    press(&mut editor, "M-b M-b");
    assert_eq!(editor.buffer().point(), "fn  main".len());
}

#[test]
fn paragraph_motion_stops_at_the_blank_lines_between_paragraphs() {
    let text = "one\ntwo\n\n  \t\nthree\n\nfour";
    let mut editor = editor_with(text);
    press(&mut editor, "C-f M-}");
    assert_eq!(editor.buffer().point(), "one\ntwo\n".len());
    press(&mut editor, "M-}");
    assert_eq!(editor.buffer().point(), "one\ntwo\n\n  \t\nthree\n".len());
    press(&mut editor, "M-}");
    assert_eq!(editor.buffer().point(), text.len());
    press(&mut editor, "M-}");
    assert_eq!(editor.echo().as_deref(), Some("End of buffer"));

    press(&mut editor, "M-{");
    assert_eq!(editor.buffer().point(), "one\ntwo\n\n  \t\nthree\n".len());
    press(&mut editor, "M-{");
    assert_eq!(editor.buffer().point(), "one\ntwo\n\n".len());
    press(&mut editor, "M-{");
    assert_eq!(editor.buffer().point(), 0);
}

#[test]
fn paragraph_motion_extends_the_region() {
    let mut editor = editor_with("one\ntwo\n\nthree");
    press(&mut editor, "C-SPC M-}");
    assert_eq!(editor.buffer().region(), Some(0.."one\ntwo\n".len()));
}

#[test]
fn a_paragraph_is_selected_or_killed() {
    let mut editor = editor_with("one\n\ntwo\nthree\n\nfour");
    press(&mut editor, "C-n C-n C-f M-h");
    assert_eq!(
        editor.buffer().region(),
        Some("one\n".len().."one\n\ntwo\nthree\n".len())
    );
    assert_eq!(editor.buffer().point(), "one\n".len());

    press(&mut editor, "C-g C-n C-f");
    editor.run_command("kill-paragraph");
    assert_eq!(contents(&editor), "one\n\nt\nfour");
}

#[test]
fn backward_kill_word_takes_one_run_at_a_time() {
    let mut editor = editor_with("тако1ую_ф22разу");
    press(&mut editor, "C-e M-DEL");
    assert_eq!(contents(&editor), "тако1ую_");
    press(&mut editor, "M-DEL");
    assert_eq!(contents(&editor), "тако1ую");
    press(&mut editor, "M-DEL");
    assert_eq!(contents(&editor), "");
}

#[test]
fn expand_region_selects_inside_then_with_each_pair_out() {
    let text = r#"f(a, ["b(c", d])"#;
    let mut editor = code_editor_with(text);
    let point = text.find("(c").unwrap();
    editor.buffer_mut().set_point(point);
    let mut expand = || {
        press(&mut editor, "M-<up>");
        editor.buffer().region().map(|region| &text[region])
    };
    assert_eq!(expand(), Some("b(c"));
    assert_eq!(expand(), Some(r#""b(c""#));
    assert_eq!(expand(), Some(r#""b(c", d"#));
    assert_eq!(expand(), Some(r#"["b(c", d]"#));
    assert_eq!(expand(), Some(r#"a, ["b(c", d]"#));
    assert_eq!(expand(), Some(r#"(a, ["b(c", d])"#));
    assert_eq!(expand(), Some(r#"(a, ["b(c", d])"#));
}

#[test]
fn contract_region_goes_back_to_point() {
    let text = "f(a, [b])";
    let mut editor = code_editor_with(text);
    let point = text.find('b').unwrap();
    editor.buffer_mut().set_point(point);
    press(&mut editor, "C-= C-= C-= C--");
    assert_eq!(
        editor.buffer().region().map(|region| &text[region]),
        Some("[b]")
    );
    press(&mut editor, "M-<down> M-<down>");
    assert_eq!(editor.buffer().region(), None);
    assert_eq!(editor.buffer().point(), point);
    press(&mut editor, "C--");
    assert_eq!(editor.buffer().point(), point);
}

#[test]
fn contract_region_forgets_expansions_after_other_commands() {
    let mut editor = code_editor_with("(a b)");
    press(&mut editor, "C-f C-= C-f");
    assert_eq!(editor.buffer().region(), Some(1..5));
    press(&mut editor, "C--");
    assert_eq!(editor.buffer().region(), Some(1..5));
}

#[test]
fn expand_region_skips_a_pair_point_only_touches() {
    let mut editor = code_editor_with("{x (y)}");
    press(&mut editor, "C-f C-f C-f C-=");
    assert_eq!(editor.buffer().region(), Some(1..6));
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
    press(&mut editor, "M-f M-f M-f M-f C-SPC C-e C-b");
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
fn quotes_close_themselves_and_are_typed_over() {
    let mut editor = code_editor_with("");
    type_text(&mut editor, r#"s = "ab";"#);
    assert_eq!(contents(&editor), r#"s = "ab";"#);
    type_text(&mut editor, r#" t = """#);
    assert_eq!(contents(&editor), r#"s = "ab"; t = """#);
    assert_eq!(editor.buffer().point(), contents(&editor).len());
}

#[test]
fn quotes_stay_single_in_strings_and_after_words() {
    let mut editor = code_editor_with(r#""it""#);
    press(&mut editor, "C-f C-f C-f");
    type_text(&mut editor, "'");
    assert_eq!(contents(&editor), r#""it'""#);
    let mut editor = code_editor_with("don");
    press(&mut editor, "C-e");
    type_text(&mut editor, "'");
    assert_eq!(contents(&editor), "don'");
}

#[test]
fn quotes_wrap_the_region_and_delete_as_a_pair() {
    let mut editor = code_editor_with("word");
    press(&mut editor, "C-SPC C-e");
    type_text(&mut editor, "'");
    assert_eq!(contents(&editor), "'word'");
    let mut editor = code_editor_with("");
    type_text(&mut editor, "\"");
    press(&mut editor, "DEL");
    assert_eq!(contents(&editor), "");
}

#[test]
fn comment_line_toggles_point_line() {
    let mut editor = code_editor_with("fn f() {\n    g();\n}");
    press(&mut editor, "C-n C-e M-;");
    assert_eq!(contents(&editor), "fn f() {\n    // g();\n}");
    assert_eq!(editor.buffer().point(), "fn f() {\n    // g();".len());
    press(&mut editor, "s-/");
    assert_eq!(contents(&editor), "fn f() {\n    g();\n}");
    press(&mut editor, "M-; C-/");
    assert_eq!(contents(&editor), "fn f() {\n    g();\n}");
}

#[test]
fn comment_line_aligns_the_regions_lines_and_skips_blank_ones() {
    let text = "if x {\n    a();\n\n        b();\n}\nc();";
    let mut editor = code_editor_with(text);
    press(&mut editor, "C-n C-SPC C-n C-n C-n");
    press(&mut editor, "M-;");
    assert_eq!(
        contents(&editor),
        "if x {\n    // a();\n\n    //     b();\n}\nc();"
    );
    assert!(editor.buffer().region().is_some());
    press(&mut editor, "M-;");
    assert_eq!(contents(&editor), text);
}

#[test]
fn comment_line_comments_all_when_some_are_not() {
    let mut editor = code_editor_with("// a\nb");
    press(&mut editor, "C-SPC M-> M-;");
    assert_eq!(contents(&editor), "// // a\n// b");
}

#[test]
fn comment_line_uses_block_comments_where_there_are_no_others() {
    let mut editor = editor_in_mode("  a { b: c; }  ", STYLE_MODE);
    press(&mut editor, "M-;");
    assert_eq!(contents(&editor), "  /* a { b: c; } */  ");
    press(&mut editor, "M-;");
    assert_eq!(contents(&editor), "  a { b: c; }  ");
}

#[test]
fn comment_line_uses_the_markup_comment_in_markup() {
    let text = "return (\n  <div>\n    <b>hi</b>\n  </div>\n);";
    let mut editor = code_editor_with(text);
    press(&mut editor, "C-n C-n M-;");
    assert_eq!(
        contents(&editor),
        "return (\n  <div>\n    {/* <b>hi</b> */}\n  </div>\n);"
    );
    press(&mut editor, "M-;");
    assert_eq!(contents(&editor), text);
    press(&mut editor, "M-< M-;");
    assert_eq!(contents(&editor), text.replacen("return", "// return", 1));
}

#[test]
fn comment_line_needs_a_comment_syntax() {
    let mut editor = editor_with("text");
    press(&mut editor, "M-;");
    assert_eq!(contents(&editor), "text");
    assert_eq!(
        editor.echo().as_deref(),
        Some("No comment syntax in this mode")
    );
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
    press(&mut editor, "M-f C-SPC M-f M-f");
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
    press(&mut editor, "M-> M-DEL M-DEL M-DEL");
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
