use std::fs;
use std::ops::Range;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use remacs_core::{Buffer, Editor, FUNDAMENTAL_MODE, Face, Settings, parse_sequence};
use remacs_editing::EditingPlugin;
use remacs_minibuffer::MinibufferPlugin;
use remacs_text::Text;

use crate::SearchPlugin;
use crate::query::Query;

#[test]
fn the_default_file_reads_as_the_defaults() {
    remacs_core::assert_default_file::<crate::project::SearchSettings>();
}

const TEXT: &str =
    "fn main() {\n    let total = sum();\n}\n\nfn sum() -> u32 {\n    Total + total\n}\n";

fn editor_at(offset: usize) -> Editor {
    let mut editor = editor_with(TEXT);
    editor.window_buffer_mut().set_point(offset);
    editor
}

fn editor_with(text: &str) -> Editor {
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    editor.add_buffer(Buffer::new("main.rs", Text::from(text), FUNDAMENTAL_MODE));
    editor
}

fn press(editor: &mut Editor, keys: &str) {
    for key in parse_sequence(keys).unwrap() {
        editor.handle_key(key);
    }
}

fn type_text(editor: &mut Editor, text: &str) {
    for character in text.chars() {
        let key = if character == ' ' {
            "SPC".to_owned()
        } else {
            character.to_string()
        };
        press(editor, &key);
    }
}

fn listed(editor: &Editor) -> Vec<(String, String)> {
    editor
        .minibuffer()
        .unwrap()
        .candidates()
        .iter()
        .map(|candidate| (candidate.annotation.clone(), candidate.label.clone()))
        .collect()
}

#[test]
fn lines_that_match_are_listed_with_the_first_after_point_selected() {
    let mut editor = editor_at(TEXT.find("fn sum").unwrap());
    press(&mut editor, "C-s");
    type_text(&mut editor, "sum");
    assert_eq!(
        listed(&editor),
        [
            ("2".into(), "let total = sum();".into()),
            ("5".into(), "fn sum() -> u32 {".into()),
        ]
    );
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(minibuffer.selected(), Some(1));
    assert_eq!(
        minibuffer.candidates()[0].matches,
        std::slice::from_ref(&(12..15))
    );

    press(&mut editor, "C-r RET");
    assert!(editor.minibuffer().is_none());
    assert_eq!(editor.window_buffer().point(), TEXT.find("sum").unwrap());
    assert_eq!(
        editor.window_buffer().mark(),
        Some(TEXT.find("fn sum").unwrap())
    );
}

#[test]
fn searching_backward_selects_the_last_match_before_point_and_s_g_steps() {
    let mut editor = editor_at(TEXT.find("fn sum").unwrap());
    press(&mut editor, "C-r");
    type_text(&mut editor, "sum");
    assert_eq!(editor.minibuffer().unwrap().selected(), Some(0));
    press(&mut editor, "s-g");
    assert_eq!(editor.minibuffer().unwrap().selected(), Some(1));
    press(&mut editor, "s-G");
    assert_eq!(editor.minibuffer().unwrap().selected(), Some(0));
}

#[test]
fn case_is_ignored_unless_the_input_has_an_uppercase_letter() {
    let mut editor = editor_at(0);
    press(&mut editor, "C-s");
    type_text(&mut editor, "total");
    let minibuffer = editor.minibuffer().unwrap();
    assert_eq!(listed(&editor).len(), 2);
    assert_eq!(minibuffer.candidates()[1].matches, [0..5, 8..13]);

    press(&mut editor, "C-a C-k");
    type_text(&mut editor, "Total");
    assert_eq!(listed(&editor), [("6".into(), "Total + total".into())]);

    press(&mut editor, "C-a C-k");
    type_text(&mut editor, "nothing");
    assert!(listed(&editor).is_empty());
    press(&mut editor, "RET");
    assert_eq!(editor.echo().as_deref(), Some("No match"));
    press(&mut editor, "C-g");
    assert_eq!(editor.window_buffer().point(), 0);
}

fn contents(editor: &Editor) -> String {
    let buffer = editor.window_buffer();
    buffer.contents(0..buffer.text().len())
}

/// Opens `M-%` from point with `from` replaced by `to`, at the first match.
fn replacing(offset: usize, from: &str, to: &str) -> Editor {
    let mut editor = editor_at(offset);
    press(&mut editor, "M-%");
    type_text(&mut editor, from);
    press(&mut editor, "RET");
    type_text(&mut editor, to);
    press(&mut editor, "RET");
    editor
}

#[test]
fn replacing_asks_at_each_match_and_undoes_at_once() {
    let mut editor = editor_at(0);
    press(&mut editor, "M-%");
    type_text(&mut editor, "total");
    assert_eq!(editor.minibuffer().unwrap().note(), Some("3 matches"));
    assert_eq!(lit(&editor).len(), 3);
    press(&mut editor, "C-g");

    let mut editor = replacing(0, "total", "count");
    assert_eq!(editor.window_buffer().point(), TEXT.find("total").unwrap());
    assert_eq!(lit(&editor)[0], (TEXT.find("total").unwrap(), true));

    press(&mut editor, "y");
    let after = contents(&editor);
    assert!(after.contains("let count = sum();"));
    assert_eq!(editor.window_buffer().point(), after.find("Total").unwrap());

    press(&mut editor, "n y");
    assert!(editor.minibuffer().is_none());
    assert!(lit(&editor).is_empty());
    assert_eq!(editor.echo().as_deref(), Some("Replaced 2 occurrences"));
    assert!(contents(&editor).contains("Total + count"));

    press(&mut editor, "C-/");
    assert_eq!(contents(&editor), TEXT);
}

#[test]
fn replacing_all_quitting_and_nothing_to_replace() {
    let start = TEXT.find("fn sum").unwrap();
    let mut editor = replacing(start, "total", "x");
    press(&mut editor, "!");
    assert_eq!(editor.echo().as_deref(), Some("Replaced 2 occurrences"));
    assert!(contents(&editor).contains("x + x") && contents(&editor).contains("let total"));

    let mut editor = replacing(0, "sum", "add");
    press(&mut editor, "y C-g");
    assert!(editor.minibuffer().is_none());
    assert_eq!(editor.echo().as_deref(), Some("Replaced 1 occurrence"));
    assert!(contents(&editor).contains("fn sum()"));

    let editor = replacing(start, "main", "start");
    assert_eq!(editor.echo().as_deref(), Some("No “main” after point"));
    assert!(editor.minibuffer().is_none());
}

/// Spans lit by the search, with the chosen one's face told apart.
fn lit(editor: &Editor) -> Vec<(usize, bool)> {
    editor
        .decorations(0..TEXT.len())
        .into_iter()
        .flatten()
        .filter_map(|span| match span.face {
            Face::SearchMatch => Some((span.range.start, false)),
            Face::SearchCurrent => Some((span.range.start, true)),
            _ => None,
        })
        .collect()
}

#[test]
fn matches_are_lit_and_point_follows_the_selection_until_quitting() {
    let mut editor = editor_at(0);
    press(&mut editor, "C-s");
    type_text(&mut editor, "total");
    let [first, second, third] = [
        TEXT.find("total").unwrap(),
        TEXT.find("Total").unwrap(),
        TEXT.rfind("total").unwrap(),
    ];
    assert_eq!(editor.window_buffer().point(), first);
    assert_eq!(
        lit(&editor),
        [(first, true), (second, false), (third, false)]
    );

    press(&mut editor, "C-s");
    assert_eq!(editor.window_buffer().point(), second);
    assert_eq!(
        lit(&editor),
        [(first, false), (second, true), (third, false)]
    );

    press(&mut editor, "C-g");
    assert!(editor.minibuffer().is_none());
    assert_eq!(editor.window_buffer().point(), 0);
    assert!(lit(&editor).is_empty());

    press(&mut editor, "C-s");
    type_text(&mut editor, "total");
    press(&mut editor, "RET");
    assert_eq!(editor.window_buffer().point(), first);
    assert!(lit(&editor).is_empty());
}

fn found(query: &str, text: &Text, range: Range<usize>) -> Vec<usize> {
    let mut starts = Vec::new();
    let query = Query::new(query, false).unwrap().unwrap();
    query.find(text, range, |matched| {
        starts.push(matched.start);
        true
    });
    starts
}

/// A Git work tree: two files with matches, an ignored one and a binary one.
fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("remacs-search-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("src/target")).unwrap();
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    let file = open(\"a\");\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/io.rs"),
        "pub fn open_file() {}\nfn open() {}\n",
    )
    .unwrap();
    fs::write(root.join("src/target/out.rs"), "open file\n").unwrap();
    fs::write(root.join("data.bin"), b"open\0file\n").unwrap();
    root.canonicalize().unwrap()
}

/// Refreshes until the prompt lists something and says nothing more is coming.
fn wait_for_lines(editor: &mut Editor) -> Vec<(String, String, String)> {
    let started = Instant::now();
    loop {
        editor.refresh();
        let minibuffer = editor.minibuffer().unwrap();
        let candidates = minibuffer.candidates();
        if !candidates.is_empty() && minibuffer.note().is_none() {
            return candidates
                .iter()
                .map(|candidate| {
                    let group = candidate.group.as_deref().unwrap_or_default().to_owned();
                    (group, candidate.annotation.clone(), candidate.label.clone())
                })
                .collect();
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no lines found"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_project_is_searched_for_lines_with_every_word_unsaved_buffers_as_they_are() {
    let root = project("words");
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    editor.visit_file(&root.join("src/main.rs")).unwrap();
    let buffer = editor.window_buffer_mut();
    buffer.set_point(buffer.text().len());
    buffer.insert("// open the file\n");

    press(&mut editor, "M-s g");
    type_text(&mut editor, "file open");
    let lines = wait_for_lines(&mut editor);
    assert_eq!(
        lines,
        [
            (
                "src/main.rs".into(),
                "2".into(),
                "let file = open(\"a\");".into()
            ),
            ("src/main.rs".into(), "4".into(), "// open the file".into()),
            (
                "src/io.rs".into(),
                "1".into(),
                "pub fn open_file() {}".into()
            ),
        ]
    );

    press(&mut editor, "C-n C-n RET");
    let buffer = editor.window_buffer();
    assert_eq!(buffer.path(), Some(root.join("src/io.rs").as_path()));
    assert_eq!(buffer.point(), "pub fn ".len());
    fs::remove_dir_all(root).unwrap();
}

/// The window buffer's file name, point, and its lit words: where each starts, whether
/// on the chosen line.
fn shown(editor: &Editor) -> (String, usize, Vec<(usize, bool)>) {
    let buffer = editor.window_buffer();
    let lit = editor
        .decorations(0..buffer.text().len())
        .into_iter()
        .flatten()
        .filter_map(|span| match span.face {
            Face::SearchMatch => Some((span.range.start, false)),
            Face::SearchCurrent => Some((span.range.start, true)),
            _ => None,
        })
        .collect();
    (buffer.name().to_owned(), buffer.point(), lit)
}

fn has_buffer(editor: &Editor, name: &str) -> bool {
    editor.buffers().iter().any(|buffer| buffer.name() == name)
}

#[test]
fn the_window_shows_the_line_selected_and_quitting_puts_it_back() {
    let root = project("preview");
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    editor.visit_file(&root.join("src/main.rs")).unwrap();

    press(&mut editor, "M-s g");
    type_text(&mut editor, "open");
    assert_eq!(wait_for_lines(&mut editor).len(), 3);
    let second_line = "pub fn open_file() {}\nfn ".len();
    assert_eq!(
        shown(&editor),
        ("io.rs".into(), 7, vec![(7, true), (second_line, false)])
    );
    press(&mut editor, "C-n");
    assert_eq!(
        shown(&editor),
        (
            "io.rs".into(),
            second_line,
            vec![(7, false), (second_line, true)]
        )
    );
    press(&mut editor, "C-n");
    let call = "fn main() {\n    let file = ".len();
    assert_eq!(shown(&editor), ("main.rs".into(), call, vec![(call, true)]));

    press(&mut editor, "C-g");
    assert_eq!(shown(&editor), ("main.rs".into(), 0, Vec::new()));
    assert!(!has_buffer(&editor, "io.rs"));

    press(&mut editor, "M-s g");
    type_text(&mut editor, "open");
    wait_for_lines(&mut editor);
    press(&mut editor, "RET");
    assert_eq!(shown(&editor), ("io.rs".into(), 7, Vec::new()));
    assert!(has_buffer(&editor, "main.rs"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_search_stops_at_the_most_lines_the_settings_allow() {
    let root = project("most");
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    let settings = [("search.toml", "project-max-matches = 2\n")]
        .map(|(name, text)| (PathBuf::from(name), text.to_owned()));
    editor.set_settings(Settings::from_files(settings));
    editor.visit_file(&root.join("src/io.rs")).unwrap();

    press(&mut editor, "M-s g");
    type_text(&mut editor, "open");
    let started = Instant::now();
    while editor.minibuffer().unwrap().note() != Some("First 2 lines") {
        assert!(started.elapsed() < Duration::from_secs(10), "never stopped");
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
    assert_eq!(editor.minibuffer().unwrap().candidates().len(), 2);
    fs::remove_dir_all(root).unwrap();
}

/// Opens `project-query-replace` in the project's main.rs and answers it, then refreshes
/// until it asks at a match or ends.
fn replacing_in_project(name: &str, from: &str, to: &str) -> (PathBuf, Editor) {
    let root = project(name);
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    editor.visit_file(&root.join("src/main.rs")).unwrap();
    press(&mut editor, "M-s %");
    type_text(&mut editor, from);
    press(&mut editor, "RET");
    type_text(&mut editor, to);
    press(&mut editor, "RET");
    wait_for_match(&mut editor);
    (root, editor)
}

fn wait_for_match(editor: &mut Editor) {
    let started = Instant::now();
    while editor
        .minibuffer()
        .is_some_and(|minibuffer| minibuffer.note() == Some("Searching…"))
    {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no match found"
        );
        thread::sleep(Duration::from_millis(10));
        editor.refresh();
    }
}

fn read(root: &std::path::Path, file: &str) -> String {
    fs::read_to_string(root.join(file)).unwrap()
}

#[test]
fn replacing_in_the_project_asks_file_by_file_saves_and_goes_back() {
    let (root, mut editor) = replacing_in_project("replace", "file", "path");
    let asked = editor.minibuffer().unwrap().note().unwrap().to_owned();
    assert!(
        ["src/main.rs", "src/io.rs"].contains(&asked.as_str()),
        "{asked}"
    );
    assert_eq!(
        shown(&editor)
            .2
            .iter()
            .filter(|(_, chosen)| *chosen)
            .count(),
        1
    );
    press(&mut editor, "y");
    wait_for_match(&mut editor);
    press(&mut editor, "y");
    wait_for_match(&mut editor);

    assert!(editor.minibuffer().is_none());
    assert_eq!(
        editor.echo().as_deref(),
        Some("Replaced 2 occurrences in 2 files")
    );
    assert_eq!(
        read(&root, "src/main.rs"),
        "fn main() {\n    let path = open(\"a\");\n}\n"
    );
    assert_eq!(
        read(&root, "src/io.rs"),
        "pub fn open_path() {}\nfn open() {}\n"
    );
    assert_eq!(read(&root, "src/target/out.rs"), "open file\n");
    assert_eq!(shown(&editor), ("main.rs".into(), 0, Vec::new()));
    assert!(!editor.window_buffer().is_modified());
    assert!(!has_buffer(&editor, "io.rs"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn replacing_everywhere_skipping_and_stopping() {
    let (root, mut editor) = replacing_in_project("everywhere", "open", "load");
    press(&mut editor, "A");
    wait_for_match(&mut editor);
    assert!(editor.minibuffer().is_none());
    assert_eq!(
        editor.echo().as_deref(),
        Some("Replaced 3 occurrences in 2 files")
    );
    assert_eq!(
        read(&root, "src/io.rs"),
        "pub fn load_file() {}\nfn load() {}\n"
    );
    fs::remove_dir_all(root).unwrap();

    let (root, mut editor) = replacing_in_project("skipping", "open", "load");
    press(&mut editor, "n q");
    assert!(editor.minibuffer().is_none());
    assert_eq!(
        editor.echo().as_deref(),
        Some("Replaced 0 occurrences in 0 files")
    );
    assert!(read(&root, "src/io.rs").contains("open"));
    assert!(read(&root, "src/main.rs").contains("open"));
    fs::remove_dir_all(root).unwrap();

    let (root, editor) = replacing_in_project("nothing", "nowhere", "here");
    assert!(editor.minibuffer().is_none());
    assert_eq!(editor.echo().as_deref(), Some("No matches in the project"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn matches_across_chunks_are_found_once_each() {
    let mut text = Text::from("world aaa");
    text.insert(0, "hello ");
    text.insert(text.len(), "aa");
    assert_eq!(found("o w", &text, 0..text.len()), [4]);
    assert_eq!(found("aa", &text, 0..text.len()), [12, 14]);
    assert_eq!(found("aa", &text, 13..text.len()), [13, 15]);
    assert_eq!(found("aa", &text, 13..14), [13]);
    assert_eq!(found("aa", &text, 14..14), [] as [usize; 0]);
}

fn found_regexp(pattern: &str, text: &Text, range: Range<usize>) -> Vec<Range<usize>> {
    let mut matches = Vec::new();
    let query = Query::new(pattern, true).unwrap().unwrap();
    query.find(text, range, |matched| {
        matches.push(matched);
        true
    });
    matches
}

#[test]
fn a_regexp_is_found_across_pieces_seeing_the_lines_around_its_range() {
    let mut text = Text::from("let a1 = 2;\nb22");
    text.insert(text.len(), "3 = a1;\n");
    assert_eq!(
        found_regexp(r"[a-z]\d+", &text, 0..text.len()),
        [4..6, 12..16, 19..21]
    );
    assert_eq!(
        found_regexp(r"^\w", &text, 1..text.len()),
        std::slice::from_ref(&(12..13))
    );
    assert_eq!(
        found_regexp(r"\b\d", &text, 5..text.len()),
        std::slice::from_ref(&(9..10))
    );
    assert_eq!(
        found_regexp("$", &text, 0..text.len()),
        [11..11, 22..22, 23..23]
    );

    let text = Text::from(TEXT);
    let total = TEXT.find("Total").unwrap();
    assert_eq!(
        found_regexp(r"\WTotal", &text, 0..TEXT.len()),
        std::slice::from_ref(&(total - 1..total + 5))
    );
    assert_eq!(found_regexp(r"\Wtotal\b", &text, 0..TEXT.len()).len(), 3);
    assert_eq!(
        Query::new("sum(", true).err().as_deref(),
        Some("Invalid regexp: unclosed group")
    );
}

#[test]
fn a_regexp_finds_what_the_regex_crate_finds_in_long_lines_and_small_pieces() {
    // Lines longer than a searched block and than a match is looked for in.
    let line: String = (0..20_000).map(|n| format!("x{n} aa ")).collect();
    let source = format!("{line}\nshort a1 é1\n\n{line}");
    let chars: Vec<char> = source.chars().collect();
    let mut pieces = Text::from("");
    for chunk in chars.chunks(997).rev() {
        pieces.insert(0, &chunk.iter().collect::<String>());
    }
    for pattern in [r"\d+", r"\ba+", "^", "$", "a*", r"é\w", r"\d\n"] {
        // On `str`, whose empty matches never split a character, as an edit there would.
        let expected: Vec<_> = regex::RegexBuilder::new(pattern)
            .case_insensitive(true)
            .multi_line(true)
            .build()
            .unwrap()
            .find_iter(&source)
            .map(|found| found.range())
            .collect();
        for text in [&Text::from(source.as_str()), &pieces] {
            assert_eq!(
                found_regexp(pattern, text, 0..text.len()),
                expected,
                "{pattern}"
            );
        }
    }
}

#[test]
fn a_regexp_search_lists_its_lines_and_m_r_switches_to_literal_text() {
    let mut editor = editor_at(0);
    press(&mut editor, "C-M-s");
    type_text(&mut editor, r"fn \w+\(\)");
    assert_eq!(
        listed(&editor),
        [
            ("1".into(), "fn main() {".into()),
            ("5".into(), "fn sum() -> u32 {".into()),
        ]
    );
    assert_eq!(
        lit(&editor),
        [(0, true), (TEXT.find("fn sum").unwrap(), false)]
    );

    press(&mut editor, "M-r");
    assert!(listed(&editor).is_empty());
    assert!(lit(&editor).is_empty());

    press(&mut editor, "C-a C-k");
    type_text(&mut editor, "sum(");
    assert_eq!(listed(&editor).len(), 2);
    press(&mut editor, "M-r");
    assert!(listed(&editor).is_empty());
    assert_eq!(
        editor.minibuffer().unwrap().note(),
        Some("Invalid regexp: unclosed group")
    );

    // The spaces a line is shown without may be part of its match.
    let mut editor = editor_at(0);
    press(&mut editor, "C-M-s");
    type_text(&mut editor, r"^\s+let");
    let candidate = &editor.minibuffer().unwrap().candidates()[0];
    assert_eq!(candidate.label, "let total = sum();");
    assert_eq!(candidate.matches, std::slice::from_ref(&(0..3)));
}

#[test]
fn a_replacement_naming_a_group_the_regexp_lacks_is_refused() {
    let query = Query::new(r"(?<name>\w+) (\d)", true).unwrap().unwrap();
    for to in ["$1 ${2}x", "${name}-$0", "$$1_x", "cost: $", "${unclosed"] {
        assert_eq!(query.check_replacement(to), Ok(()), "{to}");
    }
    assert_eq!(
        query.check_replacement("$1_new"),
        Err("No group “1_new” in the regexp; braces end a group's number, as in ${1}_new".into())
    );
    assert_eq!(
        query.check_replacement("${3}"),
        Err("No group “3” in the regexp".into())
    );

    let mut editor = editor_at(0);
    press(&mut editor, "C-M-%");
    type_text(&mut editor, r"fn (\w+)\(");
    press(&mut editor, "RET");
    type_text(&mut editor, "fn $1_new(");
    press(&mut editor, "RET");
    assert_eq!(editor.minibuffer().unwrap().input_text(), "fn $1_new(");
    assert!(editor.echo().unwrap().starts_with("No group “1_new”"));
    assert_eq!(contents(&editor), TEXT);
}

#[test]
fn a_regexp_sees_crlf_line_ends_in_project_files() {
    let words = crate::words::Words::regex(";$").unwrap().unwrap();
    let mut lines = Vec::new();
    words.lines(b"a;\r\nb\r\nc;\n", &mut Vec::new(), |range| {
        lines.push(range);
        true
    });
    assert_eq!(lines, [0..3, 7..9]);
}

#[test]
fn replacing_all_agrees_with_one_at_a_time_and_takes_groups_as_found() {
    for keys in ["y y", "!"] {
        let mut editor = editor_with("aab");
        press(&mut editor, "C-M-%");
        type_text(&mut editor, "a*");
        assert_eq!(editor.minibuffer().unwrap().note(), Some("2 matches"));
        press(&mut editor, "RET X RET");
        press(&mut editor, keys);
        assert_eq!(contents(&editor), "XbX", "{keys}");
    }

    // A match cut where the search stopped looking is replaced whole, not lost.
    let mut editor = editor_with(&"a".repeat(300_000));
    press(&mut editor, "C-M-%");
    type_text(&mut editor, "a+");
    press(&mut editor, "RET");
    type_text(&mut editor, "[$0]");
    press(&mut editor, "RET !");
    let replaced = contents(&editor);
    assert_eq!(replaced.matches('a').count(), 300_000);
    assert!(replaced.starts_with("[a") && replaced.ends_with("a]"));

    // Copied alone, the match would let `ab$` match where `a` was found.
    let mut editor = editor_with("abc");
    press(&mut editor, "C-M-%");
    type_text(&mut editor, "ab$|a");
    press(&mut editor, "RET");
    type_text(&mut editor, "[$0]");
    press(&mut editor, "RET !");
    assert_eq!(contents(&editor), "[a]bc");
}

#[test]
fn a_regexp_is_replaced_with_its_groups_and_empty_matches_are_passed_once() {
    let mut editor = editor_at(0);
    press(&mut editor, "C-M-%");
    type_text(&mut editor, r"(\w+) \+ (\w+)");
    assert_eq!(editor.minibuffer().unwrap().note(), Some("1 match"));
    press(&mut editor, "RET");
    type_text(&mut editor, "$2 + ${1}");
    press(&mut editor, "RET y");
    assert!(contents(&editor).contains("    total + Total\n"));
    assert_eq!(editor.echo().as_deref(), Some("Replaced 1 occurrence"));

    let mut editor = editor_at(0);
    press(&mut editor, "C-M-%");
    type_text(&mut editor, "^");
    press(&mut editor, "RET");
    type_text(&mut editor, "// ");
    press(&mut editor, "RET y n y q");
    assert!(contents(&editor).starts_with("// fn main() {\n    let total = sum();\n// }\n\n"));
    assert_eq!(editor.echo().as_deref(), Some("Replaced 2 occurrences"));
}

#[test]
fn the_project_is_searched_and_replaced_by_a_regexp_with_its_groups() {
    let root = project("regexp");
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    editor.visit_file(&root.join("src/main.rs")).unwrap();
    press(&mut editor, "M-s g M-r");
    type_text(&mut editor, r"fn open\w*");
    assert_eq!(
        wait_for_lines(&mut editor),
        [
            (
                "src/io.rs".into(),
                "1".into(),
                "pub fn open_file() {}".into()
            ),
            ("src/io.rs".into(), "2".into(), "fn open() {}".into()),
        ]
    );
    type_text(&mut editor, "(");
    assert_eq!(
        editor.minibuffer().unwrap().note(),
        Some("Invalid regexp: unclosed group")
    );
    press(&mut editor, "C-g M-s % M-r");
    type_text(&mut editor, r"fn (\w+)\(\)");
    press(&mut editor, "RET");
    type_text(&mut editor, "fn ${1}_new()");
    press(&mut editor, "RET");
    wait_for_match(&mut editor);
    press(&mut editor, "A");
    wait_for_match(&mut editor);
    assert_eq!(
        editor.echo().as_deref(),
        Some("Replaced 3 occurrences in 2 files")
    );
    assert_eq!(
        read(&root, "src/main.rs"),
        "fn main_new() {\n    let file = open(\"a\");\n}\n"
    );
    assert_eq!(
        read(&root, "src/io.rs"),
        "pub fn open_file_new() {}\nfn open_new() {}\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn search_results_are_edited_in_a_buffer_and_written_back_unless_changed_since() {
    let root = project("edit-results");
    let mut editor = Editor::new(&[&EditingPlugin, &MinibufferPlugin, &SearchPlugin]);
    editor.visit_file(&root.join("src/main.rs")).unwrap();
    press(&mut editor, "M-s g");
    type_text(&mut editor, "open");
    wait_for_lines(&mut editor);

    press(&mut editor, "C-x C-q");
    assert!(editor.minibuffer().is_none());
    let buffer = editor.window_buffer_mut();
    assert_eq!(buffer.name(), "*search results*");
    let listed = buffer.contents(0..buffer.text().len());
    let mut lines: Vec<_> = listed.lines().collect();
    lines.sort_unstable();
    assert_eq!(
        lines,
        [
            "src/io.rs:1:pub fn open_file() {}",
            "src/io.rs:2:fn open() {}",
            "src/main.rs:2:    let file = open(\"a\");",
        ]
    );

    buffer.set_point(0);
    buffer.delete(0..1);
    press(&mut editor, "C-c C-c");
    assert!(
        editor
            .message()
            .unwrap()
            .text
            .starts_with("Lines were added")
    );
    let edited = listed
        .replace("fn open", "fn read")
        .replace("= open", "= read");
    let buffer = editor.window_buffer_mut();
    buffer.delete(0..buffer.text().len());
    buffer.insert(&edited);
    fs::write(
        root.join("src/io.rs"),
        "pub fn open_file() {}\nfn open() { 1 }\n",
    )
    .unwrap();

    press(&mut editor, "C-c C-c");
    assert_eq!(
        editor.message().unwrap().text,
        "Wrote 2 lines; left 1 line that changed since the search"
    );
    assert!(!has_buffer(&editor, "*search results*"));
    assert!(!has_buffer(&editor, "io.rs"));
    let main = editor.window_buffer();
    assert_eq!(main.name(), "main.rs");
    assert!(!main.is_modified());
    assert_eq!(
        fs::read_to_string(root.join("src/main.rs")).unwrap(),
        "fn main() {\n    let file = read(\"a\");\n}\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("src/io.rs")).unwrap(),
        "pub fn read_file() {}\nfn open() { 1 }\n"
    );
    fs::remove_dir_all(root).unwrap();
}
