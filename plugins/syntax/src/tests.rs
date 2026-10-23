use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use remacs_core::{Buffer, Editor, Face, Settings, SyntaxContext};
use remacs_text::Text;

use super::*;

#[test]
fn the_default_file_reads_as_the_defaults() {
    remacs_core::assert_default_file::<SyntaxSettings>();
}

const GRAMMARS: &[Grammar] = &[
    Grammar {
        mode: "Rust",
        language: tree_sitter_rust::LANGUAGE,
        highlights: &[tree_sitter_rust::HIGHLIGHTS_QUERY],
    },
    Grammar {
        mode: "JavaScript",
        language: tree_sitter_javascript::LANGUAGE,
        highlights: &[tree_sitter_javascript::HIGHLIGHT_QUERY],
    },
    Grammar {
        mode: "HTML",
        language: tree_sitter_html::LANGUAGE,
        highlights: &[tree_sitter_html::HIGHLIGHTS_QUERY],
    },
];
const INJECTIONS: &[Injections] = &[Injections {
    host: "HTML",
    query: tree_sitter_html::INJECTIONS_QUERY,
    documents: true,
}];
static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);

fn rust_editor(source: &str) -> Editor {
    let mut editor = Editor::new(&[&SYNTAX]);
    editor.add_buffer(Buffer::new("main.rs", Text::from(source), "Rust"));
    editor
}

fn faces(editor: &Editor) -> Vec<(String, Face)> {
    let buffer = editor.window_buffer();
    editor
        .decorations(0..buffer.text().len())
        .concat()
        .into_iter()
        .map(|span| (buffer.contents(span.range), span.face))
        .collect()
}

/// Waits for the parser thread, as the UI does when it is notified.
fn wait_for(editor: &mut Editor, face: (&str, Face)) -> Vec<(String, Face)> {
    for _ in 0..500 {
        editor.refresh();
        let faces = faces(editor);
        if faces
            .iter()
            .any(|(text, found)| (text.as_str(), *found) == face)
        {
            return faces;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("no {face:?} in {:?}", faces(editor));
}

#[test]
fn highlights_after_the_background_parse() {
    let mut editor = rust_editor("fn main() { let x = 1; }\n");
    let faces = wait_for(&mut editor, ("main", Face::Function));
    assert_eq!(
        faces,
        [
            ("fn".into(), Face::Keyword),
            ("main".into(), Face::Function),
            ("let".into(), Face::Keyword),
            ("1".into(), Face::Constant),
        ]
    );
}

#[test]
fn edits_shift_highlights_before_the_reparse() {
    let mut editor = rust_editor("fn main() {}\n");
    wait_for(&mut editor, ("main", Face::Function));
    editor.window_buffer_mut().insert("/// Entry.\n");
    editor.refresh();
    assert!(faces(&editor).contains(&("main".into(), Face::Function)));
    wait_for(&mut editor, ("/// Entry.\n", Face::Docstring));

    editor.window_buffer_mut().undo();
    editor.refresh();
    assert_eq!(faces(&editor)[1], ("main".into(), Face::Function));
}

#[test]
fn scripts_in_html_are_highlighted_as_javascript_and_follow_edits() {
    let mut editor = Editor::new(&[&SYNTAX]);
    let page = "<p>a</p>\n<script>\nlet n = 1;\n</script>\n<script>let m = 2;</script>\n";
    editor.add_buffer(Buffer::new("page.html", Text::from(page), "HTML"));
    let shown = wait_for(&mut editor, ("let", Face::Keyword));
    let scripts: Vec<_> = shown
        .iter()
        .filter(|(_, face)| matches!(face, Face::Keyword | Face::Constant))
        .map(|(text, face)| (text.as_str(), *face))
        .collect();
    assert_eq!(
        scripts,
        [
            ("let", Face::Keyword),
            ("1", Face::Constant),
            ("let", Face::Keyword),
            ("2", Face::Constant),
        ]
    );
    assert!(shown.contains(&("p".into(), Face::Function)));

    let second = page.rfind("let").unwrap();
    editor.window_buffer_mut().set_point(second);
    editor.window_buffer_mut().insert("const k = true; ");
    wait_for(&mut editor, ("const", Face::Keyword));
    let shown = faces(&editor);
    assert!(shown.contains(&("true".into(), Face::Constant)));
    assert!(shown.contains(&("2".into(), Face::Constant)));
}

#[test]
fn embedded_languages_turn_off_and_on_with_the_settings() {
    let mut editor = Editor::new(&[&SYNTAX]);
    let page = "<script>let n = 1;</script>\n";
    editor.add_buffer(Buffer::new("page.html", Text::from(page), "HTML"));
    wait_for(&mut editor, ("let", Face::Keyword));
    let settings =
        |text: &str| Settings::from_files([(PathBuf::from("syntax.toml"), text.to_owned())]);

    editor.set_settings(settings("embedded-languages = false\n"));
    editor.refresh();
    wait_for(&mut editor, ("script", Face::Function));
    assert_eq!(editor.embedded(editor.window_buffer()), Some(Vec::new()));
    assert!(!faces(&editor).contains(&("let".into(), Face::Keyword)));

    editor.set_settings(settings(""));
    editor.refresh();
    wait_for(&mut editor, ("let", Face::Keyword));
}

#[test]
fn other_modes_are_not_parsed() {
    let mut editor = Editor::new(&[&SYNTAX]);
    editor.add_buffer(Buffer::new(
        "notes",
        Text::from("fn main() {}"),
        "Fundamental",
    ));
    editor.refresh();
    thread::sleep(Duration::from_millis(20));
    editor.refresh();
    assert!(faces(&editor).is_empty());
}

/// The context right after the first `|` in `marked`, once the tree is parsed.
fn context_after(mode: &'static str, marked: &str) -> SyntaxContext {
    let offset = marked.find('|').unwrap();
    let mut editor = Editor::new(&[&SYNTAX]);
    let text = marked.replace('|', "");
    editor.add_buffer(Buffer::new("x", Text::from(text.as_str()), mode));
    for _ in 0..500 {
        editor.refresh();
        if buffer_syntax::is_parsed(editor.window_buffer()) {
            return editor.syntax_at(offset);
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("never parsed");
}

#[test]
fn tells_strings_and_comments_from_code() {
    use SyntaxContext::{Code, Comment, Markup, String};
    for (marked, context) in [
        (r#"let s = "a|b";"#, String),
        (r#"let s = "|";"#, String),
        (r#"let s = "ab"|;"#, Code),
        (r#"let s = |"ab";"#, Code),
        ("let c = 'x|';", String),
        ("// note|\nfn f() {}", Comment),
        ("// note|", Comment),
        ("/* a| */ fn f() {}", Comment),
        ("/* a */| fn f() {}", Code),
        ("fn f() {}|", Code),
    ] {
        assert_eq!(context_after("Rust", marked), context, "{marked}");
    }
    for (marked, context) in [
        ("const s = `a|${b}`;", String),
        ("const s = `a${b|}`;", Code),
        ("const s = 'a|';", String),
        ("const a = (\n  <div>\n    <|b>x</b>\n  </div>\n);", Markup),
        ("const a = <|div>x</div>;", Markup),
        ("const a = <div>hi|</div>;", Markup),
        ("const a = <div id|=\"x\">y</div>;", Code),
        ("const a = <div>{b|}</div>;", Code),
        ("const a = <div title=\"t|\">y</div>;", String),
    ] {
        assert_eq!(context_after("JavaScript", marked), context, "{marked}");
    }
}

#[test]
fn code_outside_buffers_is_highlighted_by_mode() {
    let editor = Editor::new(&[&SYNTAX]);
    let code = "pub fn len(&self) -> usize";
    let faces: Vec<_> = editor
        .highlight_code("Rust", code)
        .into_iter()
        .map(|span| (&code[span.range], span.face))
        .collect();
    assert!(faces.contains(&("fn", Face::Keyword)));
    assert!(faces.contains(&("usize", Face::Type)));
    assert_eq!(editor.highlight_code("Python", code), []);
}

/// What is highlighted as a matching pair with point at the first `|` of `marked`, each
/// part with its offset.
fn matching_pairs(mode: &'static str, marked: &str) -> Vec<(usize, String)> {
    let point = marked.find('|').unwrap();
    let mut editor = Editor::new(&[&SYNTAX]);
    let text = marked.replacen('|', "", 1);
    editor.add_buffer(Buffer::new("x", Text::from(text.as_str()), mode));
    editor.window_buffer_mut().set_point(point);
    for _ in 0..500 {
        editor.refresh();
        if buffer_syntax::is_parsed(editor.window_buffer()) {
            let buffer = editor.window_buffer();
            return editor
                .decorations(0..buffer.text().len())
                .concat()
                .into_iter()
                .filter(|span| span.face == Face::MatchingPair)
                .map(|span| (span.range.start, buffer.contents(span.range)))
                .collect();
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("never parsed");
}

#[test]
fn brackets_beside_point_are_highlighted_with_their_partners() {
    let pairs = |marked| matching_pairs("Rust", marked);
    let brackets = |offsets: [usize; 2], text: [&str; 2]| {
        vec![(offsets[0], text[0].into()), (offsets[1], text[1].into())]
    };
    assert_eq!(pairs("fn f|() {}"), brackets([4, 5], ["(", ")"]));
    assert_eq!(pairs("fn f()| {}"), brackets([4, 5], ["(", ")"]));
    assert_eq!(pairs("fn f() {| g([1]) }"), brackets([7, 16], ["{", "}"]));
    assert_eq!(pairs("fn f() { g(|[1]) }"), brackets([11, 13], ["[", "]"]));
    assert_eq!(pairs("fn f() { g([1]|) }"), brackets([10, 14], ["(", ")"]));
    assert_eq!(pairs("fn f() { g([1]|); }"), brackets([10, 14], ["(", ")"]));
    assert_eq!(pairs("fn f() { g([1])|; }"), brackets([10, 14], ["(", ")"]));
    assert_eq!(
        pairs("fn f(x: Vec|<u8>) {}"),
        brackets([11, 14], ["<", ">"])
    );
    assert_eq!(pairs("let s = \"(|\";"), []);
    assert_eq!(pairs("fn f() { x|y }"), []);
    assert_eq!(pairs("fn f() { a <| b }"), []);
}

#[test]
fn tags_at_point_are_highlighted_with_their_partners() {
    let html = |marked| matching_pairs("HTML", marked);
    let names =
        |starts: [usize; 2], name: &str| vec![(starts[0], name.into()), (starts[1], name.into())];
    assert_eq!(html("<di|v><p>x</p></div>"), names([1, 15], "div"));
    assert_eq!(html("<div><p>x</p></div|>"), names([1, 15], "div"));
    assert_eq!(html("|<div><p>x</p></div>"), names([1, 15], "div"));
    assert_eq!(html("<div><p>x</p></div>|"), names([1, 15], "div"));
    assert_eq!(html("<div>|<p>x</p></div>"), names([6, 11], "p"));
    assert_eq!(html("<div id|=\"a\">x</div>"), []);
    // In a script, by the script's own tree.
    assert_eq!(
        html("<script>f|(1)</script>"),
        vec![(9, "(".into()), (11, ")".into())]
    );

    let jsx = |marked| matching_pairs("JavaScript", marked);
    assert_eq!(
        jsx("const a = <Lis|t.Item>x</List.Item>;"),
        names([11, 24], "List.Item")
    );
    assert_eq!(
        jsx("const a = <|>x</>;"),
        vec![(10, "<>".into()), (13, "</>".into())]
    );
    assert_eq!(
        jsx("const a = <div onClick={() => f(|)}>x</div>;"),
        vec![(31, "(".into()), (32, ")".into())]
    );
}

/// The line point is on after each of `keys` from the `|` in Rust `marked`, `|` at point.
fn sexp_moves(marked: &str, keys: &[&str]) -> Vec<String> {
    let point = marked.find('|').unwrap();
    let text = marked.replacen('|', "", 1);
    let mut editor = rust_editor(&text);
    editor.window_buffer_mut().set_point(point);
    wait_for_parse(&mut editor);
    keys.iter()
        .map(|keys| {
            for key in remacs_core::parse_sequence(keys).unwrap() {
                editor.handle_key(key);
            }
            let point = editor.window_buffer().point();
            let marked = format!("{}|{}", &text[..point], &text[point..]);
            let line = marked.lines().find(|line| line.contains('|')).unwrap();
            line.trim().to_owned()
        })
        .collect()
}

fn wait_for_parse(editor: &mut Editor) {
    for _ in 0..500 {
        editor.refresh();
        if buffer_syntax::is_reparsed(editor.window_buffer()) {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// The Rust `marked` text, `|` at point and `^` at an active mark, after each of `keys`,
/// the text parsed again before each.
fn sexp_edits(marked: &str, keys: &[&str]) -> Vec<String> {
    let point = marked.find('|').unwrap();
    let mut editor = rust_editor(&marked.replacen('|', "", 1));
    editor.window_buffer_mut().set_point(point);
    keys.iter()
        .map(|keys| {
            wait_for_parse(&mut editor);
            // A command no key runs, by its name.
            match remacs_core::parse_sequence(keys) {
                Some(keys) => {
                    for key in keys {
                        editor.handle_key(key);
                    }
                }
                None => editor.run_command(keys),
            }
            let buffer = editor.window_buffer();
            let mut marked = buffer.contents(0..buffer.text().len());
            let mark = buffer.region().map(|_| buffer.mark().unwrap());
            let point = buffer.point();
            if let Some(mark) = mark.filter(|&mark| mark > point) {
                marked.insert(mark, '^');
            }
            marked.insert(point, '|');
            if let Some(mark) = mark.filter(|&mark| mark < point) {
                marked.insert(mark, '^');
            }
            marked
        })
        .collect()
}

#[test]
fn expressions_are_killed_transposed_and_marked() {
    assert_eq!(
        sexp_edits("fn m() { f(a, |b + c, d); }", &["C-M-k"]),
        ["fn m() { f(a, |, d); }"]
    );
    assert_eq!(
        sexp_edits("fn m() { f(a, b|, d); }", &["C-M-DEL"]),
        ["fn m() { f(a, |, d); }"]
    );
    assert_eq!(
        sexp_edits("fn m() { f(a, |b); }", &["C-M-t"]),
        ["fn m() { f(b, a|); }"]
    );
    assert_eq!(
        sexp_edits("fn m() { |a; b; }", &["C-M-SPC", "C-M-SPC"]),
        ["fn m() { |a;^ b; }", "fn m() { |a; b;^ }"]
    );
}

#[test]
fn brackets_slurp_and_barf_expressions_but_not_separators() {
    assert_eq!(
        sexp_edits("fn m() { f(g(|a), b); }", &["C-)", "C-}", "C-}"]),
        [
            "fn m() { f(g(|a, b)); }",
            "fn m() { f(g(|a), b); }",
            "fn m() { f(g(|)a, b); }",
        ]
    );
    assert_eq!(
        sexp_edits("fn m() { let x = [[|a, b], c]; }", &["C-{"]),
        ["fn m() { let x = [|a, [b], c]; }"]
    );
    assert_eq!(
        sexp_edits("fn m() { let x = [a, [|b], c]; }", &["C-("]),
        ["fn m() { let x = [[a, |b], c]; }"]
    );
}

#[test]
fn lists_are_spliced_and_raised_with_what_they_close() {
    assert_eq!(
        sexp_edits("fn m() { f(g(|a, b)); }", &["M-D"]),
        ["fn m() { f(g|a, b); }"]
    );
    assert_eq!(
        sexp_edits("fn m() { f(g(a, |b)); }", &["raise-sexp"]),
        ["fn m() { f(|b); }"]
    );
}

#[test]
fn sexps_are_the_nodes_point_is_between() {
    let source = "fn main() {\n    |let total = add(1, mul(2, 3));\n    total\n}\n";
    assert_eq!(
        sexp_moves(source, &["C-M-f", "C-M-f", "C-M-b", "C-M-b"]),
        [
            "let total = add(1, mul(2, 3));|",
            "total|",
            "|total",
            "|let total = add(1, mul(2, 3));",
        ]
    );
    assert_eq!(
        sexp_moves("fn f() { tot|al }\n", &["C-M-f", "C-M-f"]),
        ["fn f() { total| }", "fn f() { total| }"]
    );
    assert_eq!(sexp_moves("fn f() { |}\n", &["C-M-f"]), ["fn f() { |}"]);
}

#[test]
fn lists_are_entered_left_and_skipped_by_their_brackets() {
    let source = "fn main() {\n    |let total = add(1, mul(2, 3));\n}\n";
    assert_eq!(
        sexp_moves(
            source,
            &["C-M-d", "C-M-n", "C-M-p", "C-M-u", "C-M-u", "C-M-u"]
        ),
        [
            "let total = add(|1, mul(2, 3));",
            "let total = add(1, mul(2, 3)|);",
            "let total = add(1, mul|(2, 3));",
            "let total = add|(1, mul(2, 3));",
            "fn main() |{",
            "fn main() |{",
        ]
    );
}
