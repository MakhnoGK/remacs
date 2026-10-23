use std::rc::Rc;

use remacs_core::{
    Buffer, Command, CommentSyntax, Editor, Key, Keymap, MessageKind, MinorKeymap, Mode, Plugin,
    Registry, SyntaxContext, parse_sequence,
};
use remacs_text::Text;

use crate::{EditingPlugin, EditingSettings};

const CODE_MODE: &str = "Code";
const STYLE_MODE: &str = "Style";

#[test]
fn the_default_file_reads_as_the_defaults() {
    remacs_core::assert_default_file::<EditingSettings>();
}

struct CodeModePlugin;

impl Plugin for CodeModePlugin {
    fn register(&self, registry: &mut Registry) {
        registry.modes.add(Mode {
            name: CODE_MODE,
            pairs: &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')],
            comment: Some(CommentSyntax {
                start: "// ",
                end: "",
            }),
            markup_comment: Some(CommentSyntax {
                start: "{/* ",
                end: " */}",
            }),
            ..Mode::default()
        });
        registry.modes.add(Mode {
            name: STYLE_MODE,
            comment: Some(CommentSyntax {
                start: "/* ",
                end: " */",
            }),
            ..Mode::default()
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
    assert_eq!(editor.message().unwrap().kind, MessageKind::Info);
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
fn word_steps_and_kills_follow_intellij() {
    let cases = [
        ("foo bar|", "M-b", "foo |bar"),
        ("foo  |  bar", "M-b", "|foo    bar"),
        ("my_list|", "M-b", "|my_list"),
        ("foo\n    |bar", "M-b", "foo|\n    bar"),
        ("main|();", "M-f", "main(|);"),
        ("foo|\n    bar", "M-f", "foo\n|    bar"),
        ("foo\n|    bar", "M-f", "foo\n    bar|"),
        ("foo| bar", "M-f", "foo bar|"),
        ("foo bar|", "M-DEL", "foo |"),
        ("foo |", "M-DEL", "|"),
        ("CARET_STOP_OPTIONS|", "M-DEL", "|"),
        ("foo();|", "M-DEL", "foo()|"),
        ("foo\n    bar|", "M-DEL", "foo\n    |"),
        ("foo\n    |", "M-DEL", "foo\n|"),
        ("foo\n|", "M-DEL", "foo|"),
        ("meta: {\n    |sentAt", "M-DEL", "meta: {\n|sentAt"),
        ("\"test\"|", "M-DEL", "|"),
        ("foo| bar", "M-d", "foo|bar"),
        ("foo,| bar", "M-d", "foo,|bar"),
        ("foo|\n    bar", "M-d", "foo|    bar"),
        ("foo|    bar", "M-d", "foo|bar"),
        ("|\"test\", x", "M-d", "|, x"),
        ("слово1 слово_2|", "M-DEL", "слово1 |"),
        ("слово1| слово_2", "M-f", "слово1 слово_2|"),
        ("foo| bar", "M-u", "foo BAR|"),
    ];
    for (before, keys, expected) in cases {
        let mut editor = editor_with(&before.replace('|', ""));
        editor.buffer_mut().set_point(before.find('|').unwrap());
        press(&mut editor, keys);
        let mut after = contents(&editor);
        after.insert(editor.buffer().point(), '|');
        assert_eq!(after, expected, "{keys} on {before:?}");
    }
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
fn word_kills_keep_quotes_single_where_no_parser_reads_strings() {
    let mut editor = Editor::new(&[&EditingPlugin]);
    editor.add_buffer(Buffer::new(
        "test",
        Text::from("\"a\" \"b\""),
        "Fundamental",
    ));
    editor.buffer_mut().set_point(2);
    press(&mut editor, "M-d");
    assert_eq!(contents(&editor), "\"a \"b\"");
    press(&mut editor, "C-e M-DEL");
    assert_eq!(contents(&editor), "\"a \"b");
}

// The `^`-marked strings of IntelliJ's `MoveToCaretStopTest.kt` (intellij-community), its
// default (macOS) caret stops.

#[test]
fn forward_word_stops_like_intellij() {
    for marked in [
        r#"doTest^(^"^test^"^,^ Direction^.^FORWARD^)^"#,
        r#"
^      assert^(^i^ <^ expectedCaretOffset^)^ {^ "^Duplicate^ carets^:^ '^$stringWithCaretStops^'^"^ }^
"#,
        r#"
^  private^ fun^ doTest^(^stringWithCaretStops^:^ String^,^
^                     direction^:^ Direction^,^
^                     isDelete^:^ Boolean^ =^ false^)^ {^
"#,
        r#"
^data^ class^ CaretStopOptions^(^@^OptionTag^(^"^BACKWARD^"^)^ val^ backwardPolicy^:^ CaretStopPolicy^,^
^                            @^OptionTag^(^"^FORWARD^"^)^ val^ forwardPolicy^:^ CaretStopPolicy^)^ {^
"#,
        r#"
^  public^ void^ setCaretStopOptions^(^@^NotNull^ CaretStopOptions^ options^)^ {^
^    myOsSpecificState^.^CARET_STOP_OPTIONS^ =^ options^;^
^  }^
"#,
        r#"
^my_list^ =^ [^"^one^"^,^ "^two^"^,^ "^three^"^,^
^           "^four^"^,^ "^five^"^,^
^           "^six^"^.^six^,^ "^seven^"^seven^,^
^           eight^"^eight^"^,^ nine^/^"^nine^"^,^
^           "^...^"^"^ten^"^,^ (^"^...^"^"^eleven^"^"^...^"^)^,^ "^twelve^"^"^...^"^]^
"#,
        r#"
^  [^  "^word^"^"^...^"^]^  ,^
^  [^"^word^"^  "^...^"^]^  ,^
^  [^"^word^"^"^...^"^  ]^  ,^
^  [^ "^word^"^"^...^"^ ]^  ,^
^  [^"^  word^"^"^...^"^]^  ,^
^  [^"^ word^ "^"^...^"^]^  ,^
^  [^"^word^  "^"^...^"^]^  ,^
^  [^"^word^"^"^  ...^"^]^  ,^
^  [^"^word^"^"^ ...^ "^]^  ,^
^  [^"^word^"^"^...^  "^]^  ,^
"#,
    ] {
        assert_caret_stops("forward-word", true, marked);
    }
}

#[test]
fn backward_word_stops_like_intellij() {
    for marked in [
        r#"^doTest^(^"^test^"^, ^Direction^.^FORWARD^)"#,
        r#"
      ^assert^(^i ^< ^expectedCaretOffset^) ^{ ^"^Duplicate ^carets^: ^'^$stringWithCaretStops^'^" ^}^
"#,
        r#"
  ^private ^fun ^doTest^(^stringWithCaretStops^: ^String^,^
                     ^direction^: ^Direction^,^
                     ^isDelete^: ^Boolean ^= ^false^) ^{^
"#,
        r#"
^data ^class ^CaretStopOptions^(^@^OptionTag^(^"^BACKWARD^"^) ^val ^backwardPolicy^: ^CaretStopPolicy^,^
                            ^@^OptionTag^(^"^FORWARD^"^) ^val ^forwardPolicy^: ^CaretStopPolicy^) ^{^
"#,
        r#"
  ^public ^void ^setCaretStopOptions^(^@^NotNull ^CaretStopOptions ^options^) ^{^
    ^myOsSpecificState^.^CARET_STOP_OPTIONS ^= ^options^;^
  ^}^
"#,
        r#"
^my_list ^= ^[^"^one^"^, ^"^two^"^, ^"^three^"^,^
           ^"^four^"^, ^"^five^"^,^
           ^"^six^"^.^six^, ^"^seven^"^seven^,^
           ^eight^"^eight^"^, ^nine^/^"^nine^"^,^
           ^"^...^"^"^ten^"^, ^(^"^...^"^"^eleven^"^"^...^"^)^, ^"^twelve^"^"^...^"^]]^
"#,
        r#"
  ^[  ^"^word^"^"^...^"^]  ^,^
  ^[^"^word^"  ^"^...^"^]  ^,^
  ^[^"^word^"^"^...^"  ^]  ^,^
  ^[ ^"^word^"^"^...^" ^]  ^,^
  ^[^"  ^word^"^"^...^"^]  ^,^
  ^[^" ^word ^"^"^...^"^]  ^,^
  ^[^"^word  ^"^"^...^"^]  ^,^
  ^[^"^word^"^"  ^...^"^]  ^,^
  ^[^"^word^"^" ^... ^"^]  ^,^
  ^[^"^word^"^"^...  ^"^]  ^,^
"#,
    ] {
        assert_caret_stops("backward-word", false, marked);
    }
}

#[test]
fn kill_word_stops_like_intellij() {
    for marked in [
        r#"doTest^(^"`test`"^,^ ^Direction^.^FORWARD^)^"#,
        r#"
^      ^assert^(^i^ ^<^ ^expectedCaretOffset^)^ ^{^ ^"^Duplicate^ ^carets^:^ ^'^$stringWithCaretStops^'^"^ ^}^
"#,
        r#"
^  ^private^ ^fun^ ^doTest^(^stringWithCaretStops^:^ ^String^,^
^                     ^direction^:^ ^Direction^,^
^                     ^isDelete^:^ ^Boolean^ ^=^ ^false^)^ ^{^
"#,
        r#"
^data^ ^class^ ^CaretStopOptions^(^@^OptionTag^(^"`BACKWARD`"^)^ ^val^ ^backwardPolicy^:^ ^CaretStopPolicy^,^
^                            ^@^OptionTag^(^"`FORWARD`"^)^ ^val^ ^forwardPolicy^:^ ^CaretStopPolicy^)^ ^{^
"#,
        r#"
^  ^public^ ^void^ ^setCaretStopOptions^(^@^NotNull^ ^CaretStopOptions^ ^options^)^ ^{^
^    ^myOsSpecificState^.^CARET_STOP_OPTIONS^ ^=^ ^options^;^
^  ^}^
"#,
        r#"
^my_list^ ^=^ ^[^"`one`"^,^ ^"`two`"^,^ ^"`three`"^,^
^           ^"`four`"^,^ ^"`five`"^,^
^           ^"`six`"^.^six^,^ ^"`seven`"^seven^,^
^           ^eight^"`eight`"^,^ ^nine^/^"`nine`"^,^
^           ^"`...`"^"`ten`"^,^ ^(^"`...`"^"`eleven`"^"`...`"^)^,^ ^"`twelve`"^"`...`"^]^
"#,
    ] {
        assert_caret_stops("kill-word", true, marked);
    }
}

#[test]
fn backward_kill_word_stops_like_intellij() {
    for marked in [
        r#"^doTest^(^"`test`"^, ^Direction^.^FORWARD^)"#,
        r#"
^      ^assert^(^i ^< ^expectedCaretOffset^) ^{ ^"^Duplicate ^carets^: ^'^$stringWithCaretStops^'^" ^}^
"#,
        r#"
^  ^private ^fun ^doTest^(^stringWithCaretStops^: ^String^,^
^                     ^direction^: ^Direction^,^
^                     ^isDelete^: ^Boolean ^= ^false^) ^{^
"#,
        r#"
^data ^class ^CaretStopOptions^(^@^OptionTag^(^"`BACKWARD`"^) ^val ^backwardPolicy^: ^CaretStopPolicy^,^
^                            ^@^OptionTag^(^"`FORWARD`"^) ^val ^forwardPolicy^: ^CaretStopPolicy^) ^{^
"#,
        r#"
^  ^public ^void ^setCaretStopOptions^(^@^NotNull ^CaretStopOptions ^options^) ^{^
^    ^myOsSpecificState^.^CARET_STOP_OPTIONS ^= ^options^;^
^  ^}^
"#,
        r#"
^my_list ^= ^[^"`one`"^, ^"`two`"^, ^"`three`"^,^
^           ^"`four`"^, ^"`five`"^,^
^           ^"`six`"^.^six^, ^"`seven`"^seven^,^
^           ^eight^"`eight`"^, ^nine^/^"`nine`"^,^
^           ^"`...`"^"`ten`"^, ^(^"`...`"^"`eleven`"^"`...`"^)^, ^"`twelve`"^"`...`"^]^
"#,
    ] {
        assert_caret_stops("backward-kill-word", false, marked);
    }
}

/// Runs `command` from every offset of `marked` without its marks and checks it reaches
/// the nearest stop in its direction; a kill skips the `` ` `` stops inside a quoted word
/// unless it starts at one, as `MoveToCaretStopTest.nextTo` does.
fn assert_caret_stops(command: &str, forward: bool, marked: &str) {
    let (text, stops) = caret_stops(marked);
    let offsets: Vec<usize> = if forward {
        (0..text.len()).collect()
    } else {
        (1..=text.len()).rev().collect()
    };
    let kills = command.contains("kill");
    for offset in offsets {
        let mut editor = code_editor_with(&text);
        editor.buffer_mut().set_point(offset);
        editor.run_command(command);
        let stop = nearest_stop(&stops, offset, forward, kills);
        let mut shown = text.clone();
        let (start, end) = (offset.min(stop), offset.max(stop));
        let expected = if kills {
            shown.replace_range(start..end, "");
            (shown, start)
        } else {
            (text.clone(), stop)
        };
        assert_eq!(
            (contents(&editor), editor.buffer().point()),
            expected,
            "{command} from {offset} in {text:?}"
        );
    }
}

/// `marked` without its marks, and the offset of each `^` or `` ` `` stop, whether it is
/// inside a quoted word; the ends of the text are stops.
fn caret_stops(marked: &str) -> (String, Vec<(usize, bool)>) {
    let marked = format!("^{}^", marked.trim_matches(['^', '`']));
    let mut text = String::new();
    let mut stops = Vec::new();
    for character in marked.chars() {
        match character {
            '^' | '`' => stops.push((text.len(), character == '`')),
            _ => text.push(character),
        }
    }
    (text, stops)
}

fn nearest_stop(stops: &[(usize, bool)], offset: usize, forward: bool, kills: bool) -> usize {
    let from_inside = match stops.binary_search_by_key(&offset, |&(at, _)| at) {
        Ok(index) => stops[index].1,
        Err(index) => index > 0 && index < stops.len() && stops[index].1 && stops[index - 1].1,
    };
    let reachable = |&&(_, inside): &&(usize, bool)| !kills || from_inside || !inside;
    let mut candidates = stops.iter().filter(reachable);
    let stop = if forward {
        candidates.find(|&&(at, _)| at > offset)
    } else {
        candidates.rev().find(|&&(at, _)| at < offset)
    };
    stop.unwrap().0
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
    editor.buffer_mut().set_point("let x = ".len());
    press(&mut editor, "C-SPC C-e C-b");
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
    let buffer = editor.buffer();
    assert!(editor.viewport.contains(buffer.point_line(), &buffer.shown_lines()));
}

#[test]
fn recentering_cycles_through_middle_top_and_bottom() {
    let lines: String = (0..100).map(|line| format!("{line}\n")).collect();
    let mut editor = editor_with(&lines);
    editor.viewport.margins = (1, 1);
    for _ in 0..50 {
        press(&mut editor, "C-n");
    }
    let mut first_lines = || {
        press(&mut editor, "C-l");
        editor.viewport.first_line
    };
    assert_eq!(
        [first_lines(), first_lines(), first_lines(), first_lines()],
        [45, 49, 42, 45]
    );
}

#[test]
fn moving_to_window_lines_cycles_through_middle_top_and_bottom() {
    let lines: String = (0..100).map(|line| format!("{line}\n")).collect();
    let mut editor = editor_with(&lines);
    editor.viewport.margins = (1, 1);
    editor.viewport.first_line = 20;
    press(&mut editor, "C-v C-v");
    let first_line = editor.viewport.first_line;
    let mut point_lines = || {
        press(&mut editor, "M-r");
        assert_eq!(editor.viewport.first_line, first_line);
        editor.buffer().point_line()
    };
    let (top, bottom) = (first_line + 1, first_line + 8);
    assert_eq!(
        [point_lines(), point_lines(), point_lines(), point_lines()],
        [first_line + 5, top, bottom, first_line + 5]
    );
}

#[test]
fn popping_the_mark_goes_back_through_earlier_marks() {
    let mut editor = editor_with("one two three four");
    press(&mut editor, "M-f C-SPC C-g M-f C-SPC C-g M->");
    insert_at_start(&mut editor, "zero ");
    let point = |editor: &mut Editor, keys: &str| {
        press(editor, keys);
        editor.buffer().point()
    };
    // M-> pushed point, 12, which was the mark already.
    assert_eq!(point(&mut editor, "C-u C-SPC"), 12);
    assert_eq!(point(&mut editor, "C-SPC"), 8);
    assert_eq!(point(&mut editor, "C-SPC"), 12);
    assert!(!editor.buffer().is_mark_active());
    press(&mut editor, "C-f C-SPC");
    assert!(editor.buffer().is_mark_active());
    assert_eq!(editor.buffer().mark(), Some(13));
}

fn insert_at_start(editor: &mut Editor, text: &str) {
    let buffer = editor.buffer_mut();
    let point = buffer.point();
    buffer.set_point(0);
    buffer.insert(text);
    buffer.set_point(point + text.len());
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
    assert_eq!(editor.kill_ring.newest(), Some("\nthree"));
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

fn answer(editor: &mut Editor, input: &str) {
    editor.minibuffer_mut().unwrap().set_input(input);
    if let Err(error) = editor.exit_minibuffer(true) {
        editor.set_error(error.0);
    }
}

#[test]
fn transpose_swaps_characters_words_and_lines() {
    let mut editor = editor_with("abc\nfoo bar\nlast");
    press(&mut editor, "C-f C-t");
    assert_eq!(contents(&editor), "bac\nfoo bar\nlast");
    assert_eq!(editor.buffer().point(), 2);
    press(&mut editor, "C-e C-t");
    assert_eq!(contents(&editor), "bca\nfoo bar\nlast");
    press(&mut editor, "C-n C-a M-f M-t");
    assert_eq!(contents(&editor), "bca\nbar foo\nlast");
    assert_eq!(editor.buffer().point(), 11);
    press(&mut editor, "C-x C-t");
    assert_eq!(contents(&editor), "bar foo\nbca\nlast");
    assert_eq!(editor.buffer().point(), 12);
    press(&mut editor, "M-< C-x C-t");
    assert_eq!(editor.message().unwrap().kind, MessageKind::Error);
}

#[test]
fn case_commands_change_the_next_word_or_the_region() {
    let mut editor = editor_with("hello wORLD again");
    press(&mut editor, "M-u");
    assert_eq!(contents(&editor), "HELLO wORLD again");
    press(&mut editor, "M-c");
    assert_eq!(contents(&editor), "HELLO World again");
    press(&mut editor, "C-a C-SPC C-e M-l");
    assert_eq!(contents(&editor), "hello world again");
    press(&mut editor, "C-a C-SPC C-e M-c");
    assert_eq!(contents(&editor), "Hello World Again");
}

#[test]
fn zap_kills_up_to_a_character_read_after_the_key() {
    let mut editor = editor_with("one, two; жук");
    press(&mut editor, "M-z");
    type_text(&mut editor, ",");
    assert_eq!(contents(&editor), " two; жук");
    assert_eq!(editor.kill_ring.newest(), Some("one,"));
    press(&mut editor, "M-Z ж");
    assert_eq!(contents(&editor), "жук");
    press(&mut editor, "M-z q");
    assert_eq!(contents(&editor), "жук");
    assert_eq!(editor.message().unwrap().kind, MessageKind::Error);
    press(&mut editor, "M-z C-g C-f");
    assert_eq!(editor.buffer().point(), "ж".len());
    editor.run_command("flush-lines");
    press(&mut editor, "M-z");
    assert_eq!(editor.message().unwrap().kind, MessageKind::Error);
    press(&mut editor, "C-g C-f");
    assert_eq!(contents(&editor), "жук");
}

#[test]
fn join_line_leaves_one_space_unless_by_a_bracket() {
    let mut editor = editor_with("call(\n    one  \n   two\n)");
    press(&mut editor, "C-n C-n M-^");
    assert_eq!(contents(&editor), "call(\n    one two\n)");
    assert_eq!(editor.buffer().point(), 13);
    press(&mut editor, "M-< C-SPC M-> M-^");
    assert_eq!(contents(&editor), "call(one two)");
}

#[test]
fn cycle_spacing_goes_from_one_space_to_none_and_back() {
    let mut editor = editor_with("a  \t b");
    press(&mut editor, "C-f C-f M-SPC");
    assert_eq!(contents(&editor), "a b");
    press(&mut editor, "M-SPC");
    assert_eq!(contents(&editor), "ab");
    press(&mut editor, "M-SPC");
    assert_eq!(contents(&editor), "a  \t b");
    assert_eq!(editor.buffer().point(), 2);
}

#[test]
fn duplicate_copies_the_line_or_the_region_and_goes_to_the_copy() {
    let mut editor = editor_with("one\ntwo");
    press(&mut editor, "C-f s-d");
    assert_eq!(contents(&editor), "one\none\ntwo");
    assert_eq!(editor.buffer().point(), 5);
    press(&mut editor, "M-> C-SPC C-b C-b s-d");
    assert_eq!(contents(&editor), "one\none\ntwowo");
    assert_eq!(editor.buffer().region(), Some(11..13));
}

#[test]
fn move_line_takes_the_line_or_the_region_lines_past_the_next() {
    let mut editor = editor_with("a\nbb\ncc\nd");
    press(&mut editor, "C-n C-f M-S-<down>");
    assert_eq!(contents(&editor), "a\ncc\nbb\nd");
    assert_eq!(editor.buffer().point(), 6);
    press(&mut editor, "C-SPC C-p M-S-<up>");
    assert_eq!(contents(&editor), "cc\nbb\na\nd");
    assert_eq!(editor.buffer().region(), Some(1..4));
    press(&mut editor, "M-S-<up>");
    assert_eq!(editor.message().unwrap().kind, MessageKind::Error);
}

#[test]
fn line_commands_act_on_the_region_lines_or_the_buffer() {
    let mut editor = editor_with("c\na\nb\na\n");
    press(&mut editor, "M-x");
    editor.run_command("sort-lines");
    assert_eq!(contents(&editor), "a\na\nb\nc\n");
    editor.run_command("delete-duplicate-lines");
    assert_eq!(contents(&editor), "a\nb\nc\n");
    press(&mut editor, "C-n C-SPC M->");
    editor.run_command("flush-lines");
    answer(&mut editor, "^c");
    assert_eq!(contents(&editor), "a\nb\n");
    editor.run_command("keep-lines");
    answer(&mut editor, "b");
    assert_eq!(contents(&editor), "b\n");
    editor.run_command("keep-lines");
    answer(&mut editor, "(");
    assert!(editor.echo().unwrap().starts_with("Invalid regexp"));
    let mut editor = editor_with("a\nx\ny\nb");
    press(&mut editor, "C-n C-SPC C-n C-e");
    editor.run_command("flush-lines");
    answer(&mut editor, ".");
    assert_eq!(contents(&editor), "a\nb");
}

#[test]
fn align_regexp_lines_up_the_first_match_or_every_match() {
    let mut editor = editor_with("a = 1 = x\nbbb=2 = yy\nnone\n");
    press(&mut editor, "C-SPC M-> C-x \\");
    answer(&mut editor, "=");
    assert_eq!(contents(&editor), "a   = 1 = x\nbbb =2 = yy\nnone\n");
    press(&mut editor, "M-< C-SPC M-> C-x |");
    answer(&mut editor, "=");
    assert_eq!(contents(&editor), "a   = 1 = x\nbbb =2  = yy\nnone\n");
    let aligned = contents(&editor);
    press(&mut editor, "M-< C-SPC M-> C-x |");
    answer(&mut editor, r"\b");
    assert_eq!(contents(&editor), aligned);
    press(&mut editor, "M-< C-SPC M-> C-x |");
    answer(&mut editor, "x?");
    assert_eq!(
        editor.echo().as_deref(),
        Some("The regexp matches empty text")
    );
}

fn cursor_points(editor: &Editor) -> Vec<usize> {
    editor
        .buffer()
        .cursors()
        .iter()
        .map(|cursor| cursor.point)
        .collect()
}

#[test]
fn typing_and_deleting_happen_at_every_cursor_as_one_undo() {
    let mut editor = editor_with("foo(1)\nfoo(2)\nfoo(3)");
    press(&mut editor, "C-SPC C-f C-f C-f C-> C->");
    assert_eq!(cursor_points(&editor), [10, 17]);
    press(&mut editor, "C-<");
    assert!(editor.echo().unwrap().starts_with("No more matches"));
    press(&mut editor, "C-g");
    assert_eq!(cursor_points(&editor), [10, 17]);
    type_text(&mut editor, "bar");
    press(&mut editor, "DEL C-e");
    assert_eq!(contents(&editor), "fooba(1)\nfooba(2)\nfooba(3)");
    assert_eq!(cursor_points(&editor), [17, 26]);
    press(&mut editor, "C-/");
    assert_eq!(contents(&editor), "foobar(1)\nfoobar(2)\nfoobar(3)");
    press(&mut editor, "C-/");
    assert_eq!(contents(&editor), "foo(1)\nfoo(2)\nfoo(3)");
    press(&mut editor, "C-g");
    assert!(editor.buffer().cursors().is_empty());
}

#[test]
fn clipboard_text_from_elsewhere_yanks_at_every_cursor() {
    let mut editor = editor_with("a1 b\na2 b\na3 b");
    press(&mut editor, "C-SPC C-n C-n C-S-c C-S-c M-d");
    // remacs's own kill coming back from the clipboard leaves the cursors' kills alone.
    let own = editor.kill_ring.newest().unwrap().to_owned();
    editor.receive_clipboard(own);
    press(&mut editor, "C-y");
    assert_eq!(contents(&editor), "a1 b\na2 b\na3 b");
    press(&mut editor, "C-a M-d");
    editor.receive_clipboard("foo".into());
    press(&mut editor, "C-y");
    assert_eq!(contents(&editor), "foo b\nfoo b\nfoo b");
}

#[test]
fn undo_and_redo_put_cursors_back_with_the_text() {
    let mut editor = editor_with("ab\nab\nab");
    press(&mut editor, "C-SPC C-n C-n C-S-c C-S-c");
    assert_eq!(cursor_points(&editor), [0, 3]);
    type_text(&mut editor, "x");
    assert_eq!(cursor_points(&editor), [1, 5]);
    press(&mut editor, "C-/");
    assert_eq!(contents(&editor), "ab\nab\nab");
    assert_eq!(cursor_points(&editor), [0, 3]);
    press(&mut editor, "C-?");
    assert_eq!(contents(&editor), "xab\nxab\nxab");
    assert_eq!(cursor_points(&editor), [1, 5]);
    press(&mut editor, "C-/");
    type_text(&mut editor, "y");
    assert_eq!(contents(&editor), "yab\nyab\nyab");
}

#[test]
fn undo_neither_brings_back_removed_cursors_nor_removes_new_ones() {
    let mut editor = editor_with("ab\nab\nab");
    press(&mut editor, "C-SPC C-n C-n C-S-c C-S-c");
    type_text(&mut editor, "x");
    press(&mut editor, "C-g C-/");
    assert_eq!(contents(&editor), "ab\nab\nab");
    assert!(editor.buffer().cursors().is_empty());
    type_text(&mut editor, "y");
    press(&mut editor, "C-a C-< C-<");
    assert_eq!(cursor_points(&editor), [0, 3]);
    press(&mut editor, "C-/");
    assert_eq!(contents(&editor), "ab\nab\nab");
    assert_eq!(cursor_points(&editor), [0, 3]);
}

#[test]
fn cursors_a_run_once_command_moved_together_type_once() {
    let mut editor = editor_with("ab ab ab\nzz");
    press(&mut editor, "C-SPC C-f C-f C-c C-<");
    assert_eq!(cursor_points(&editor), [5, 8]);
    press(&mut editor, "C-g M-<");
    editor.run_command("flush-lines");
    answer(&mut editor, "ab");
    type_text(&mut editor, "x");
    assert_eq!(contents(&editor), "xzz");
}

#[test]
fn each_cursor_kills_and_yanks_its_own_text() {
    let mut editor = editor_with("a1 b\na2 b\na3 b");
    press(&mut editor, "C-SPC C-n C-n C-S-c C-S-c");
    assert_eq!(cursor_points(&editor), [0, 5]);
    press(&mut editor, "M-d C-e C-y");
    assert_eq!(contents(&editor), " ba1\n ba2\n ba3");
    assert_eq!(editor.kill_ring.newest(), Some("a3"));
}

#[test]
fn mark_all_and_skip_put_cursors_on_matches_of_the_region() {
    let mut editor = editor_with("x ab ab ab ab");
    press(&mut editor, "C-f C-f C-SPC C-f C-f C-c C-<");
    assert_eq!(cursor_points(&editor), [7, 10, 13]);
    press(&mut editor, "C-g C-g M-< C-f C-f C-SPC C-f C-f C-> C-c C->");
    assert_eq!(cursor_points(&editor), [10]);
    assert_eq!(editor.buffer().cursors()[0].region(), Some(8..10));
}

#[test]
fn cursors_stop_at_max_cursors() {
    let mut editor = editor_with("x ab ab ab ab");
    editor.plugin_state::<crate::EditingSettings>().max_cursors = 3;
    press(&mut editor, "C-f C-f C-SPC C-f C-f C-c C-<");
    assert!(editor.buffer().cursors().is_empty());
    assert_eq!(
        editor.echo().as_deref(),
        Some("4 cursors would be over max-cursors, 3")
    );
    press(&mut editor, "C-> C->");
    assert_eq!(cursor_points(&editor), [7, 10]);
    press(&mut editor, "C->");
    assert_eq!(cursor_points(&editor), [7, 10]);
    let mut editor = editor_with("x ab ab ab");
    editor.plugin_state::<crate::EditingSettings>().max_cursors = 3;
    press(&mut editor, "C-f C-f C-SPC C-f C-f C-> C-> C-c C-<");
    assert_eq!(editor.echo().as_deref(), Some("3 cursors"));
    assert_eq!(cursor_points(&editor), [7, 10]);
}

#[test]
fn iedit_puts_cursors_on_whole_symbols_at_point_offset() {
    let mut editor = editor_with("let ab = ab + abc;\nab");
    press(&mut editor, "C-f C-f C-f C-f C-f C-;");
    assert_eq!(cursor_points(&editor), [10, 20]);
    type_text(&mut editor, "Z");
    assert_eq!(contents(&editor), "let aZb = aZb + abc;\naZb");
    press(&mut editor, "C-;");
    assert!(editor.buffer().cursors().is_empty());
}

/// Stands in for a popup registered after editing, with C-g to close it.
struct PopupPlugin;

#[derive(Default)]
struct PopupShown(bool);

impl Plugin for PopupPlugin {
    fn register(&self, registry: &mut Registry) {
        registry
            .commands
            .add(Command::new("popup-quit", "Close the popup", |editor| {
                editor.plugin_state::<PopupShown>().0 = false;
                Ok(())
            }));
        let mut keymap = Keymap::default();
        keymap.bind("C-g", "popup-quit");
        registry.minor_keymaps.push(MinorKeymap {
            active: |editor| {
                editor
                    .find_plugin_state::<PopupShown>()
                    .is_some_and(|shown| shown.0)
            },
            keymap,
        });
    }
}

#[test]
fn c_g_closes_a_popup_before_it_removes_cursors() {
    let mut editor = Editor::new(&[&EditingPlugin, &PopupPlugin]);
    editor.add_buffer(Buffer::new("test", Text::from("a\nb\nc"), "Fundamental"));
    press(&mut editor, "C-> C->");
    editor.plugin_state::<PopupShown>().0 = true;
    press(&mut editor, "C-g");
    assert!(!editor.plugin_state::<PopupShown>().0);
    assert_eq!(cursor_points(&editor), [2, 4]);
    press(&mut editor, "C-g");
    assert!(editor.buffer().cursors().is_empty());
}

#[test]
fn mark_next_without_a_region_adds_a_cursor_on_the_next_line() {
    let mut editor = editor_with("abc\nd\nefg");
    press(&mut editor, "C-f C-f C-> C->");
    assert_eq!(cursor_points(&editor), [5, 8]);
    press(&mut editor, "C->");
    assert_eq!(editor.message().unwrap().kind, MessageKind::Error);
}

#[test]
fn rectangles_are_killed_yanked_opened_and_replaced_by_columns() {
    let mut editor = editor_with("abcd\nef\nijkl");
    press(&mut editor, "C-f C-SPC C-n C-n C-f C-f C-x r k");
    assert_eq!(contents(&editor), "ad\ne\nil");
    assert_eq!(editor.buffer().point(), 1);
    press(&mut editor, "M-> C-x r y");
    assert_eq!(contents(&editor), "ad\ne\nilbc\n  f \n  jk");
    press(&mut editor, "M-< C-SPC C-n C-x r o");
    assert_eq!(contents(&editor), "ad\ne\nilbc\n  f \n  jk");
    press(&mut editor, "C-f C-SPC C-n C-x r t");
    answer(&mut editor, "<>");
    assert_eq!(contents(&editor), "a<>d\ne<>\nilbc\n  f \n  jk");
    press(&mut editor, "M-< C-SPC C-n C-f C-x r o");
    assert_eq!(contents(&editor), " a<>d\n e<>\nilbc\n  f \n  jk");
}

#[test]
fn rectangle_mark_mode_makes_kill_and_yank_act_on_columns() {
    let mut editor = editor_with("abc\ndef");
    press(&mut editor, "C-f C-x SPC C-n C-f");
    assert!(editor.buffer().is_rectangle());
    press(&mut editor, "M-w");
    assert_eq!(editor.kill_ring.newest(), Some("b\ne"));
    assert!(!editor.buffer().is_rectangle());
    press(&mut editor, "C-SPC C-f");
    assert!(!editor.buffer().is_rectangle());
    press(&mut editor, "C-g M-< C-y");
    assert_eq!(contents(&editor), "babc\nedef");
    press(&mut editor, "M-< C-x SPC C-n C-f DEL");
    assert_eq!(contents(&editor), "abc\ndef");
}
