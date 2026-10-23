//! Built-in plugin: major modes, chosen by file name or extension, their tree-sitter
//! grammars and language servers.

mod grammars;
mod injections;
mod modes;
mod servers;

use remacs_core::{Plugin, Registry};

pub use grammars::{GRAMMARS, INDENTS, LOCALS};
pub use injections::INJECTIONS;
pub use modes::PRETTIER_MODES;
pub use servers::SERVERS;

pub struct LanguagesPlugin;

impl Plugin for LanguagesPlugin {
    fn register(&self, registry: &mut Registry) {
        for &mode in modes::MODES.iter() {
            registry.modes.add(mode);
        }
    }
}

#[cfg(test)]
mod fold_tests;
#[cfg(test)]
mod indent_tests;
#[cfg(test)]
mod locals_tests;

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use remacs_core::{Buffer, Editor, Face, Mode, Modes, SyntaxContext};
    use remacs_editing::EditingPlugin;
    use remacs_syntax::SyntaxPlugin;
    use remacs_text::Text;
    use tree_sitter::{Language, Query};

    use super::*;
    use crate::modes::{
        BASH, C, CMAKE, CPP, CSHARP, CSS, DART, DIFF, ELIXIR, GIT_COMMIT, GO, GRAPHQL, HASKELL,
        HCL, HTML, JAVA, JAVASCRIPT, JSON, KOTLIN, LUA, MAKE, MARKDOWN, MODES, NIX, OBJC, PHP,
        PROTOBUF, PYTHON, RUBY, RUST, SCALA, SCSS, SQL, SVELTE, SWIFT, TOML, TSX, TYPESCRIPT, VUE,
        XML, YAML, ZIG,
    };

    #[test]
    fn every_mode_has_a_grammar_whose_queries_compile() {
        for grammar in GRAMMARS {
            let known = MODES.iter().any(|mode| mode.name == grammar.mode)
                || grammars::INJECTED_ONLY.contains(&grammar.mode);
            assert!(known, "no {} in languages.toml", grammar.mode);
        }
        for &Mode { name: mode, .. } in MODES.iter() {
            let grammar = GRAMMARS
                .iter()
                .find(|grammar| grammar.mode == mode)
                .unwrap();
            let language = Language::new(grammar.language);
            let query = Query::new(&language, &grammar.highlights.concat())
                .unwrap_or_else(|error| panic!("{mode}: {error}"));
            for name in query.capture_names() {
                assert!(remacs_syntax::known_capture(name), "{mode}: @{name}");
            }
        }
    }

    #[test]
    fn editing_in_html_follows_the_language_at_point() {
        static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);
        let page = "<p>hi</p>\n<style>\na { color: red; }\n</style>\n<script>\nlet s = \"x\";\n</script>\n";
        let mut editor = Editor::new(&[&LanguagesPlugin, &EditingPlugin, &SYNTAX]);
        editor.add_buffer(Buffer::new("page.html", Text::from(page), "HTML"));
        let css = page.find("color").unwrap();
        let start = Instant::now();
        while editor.mode_at(css).map(|mode| mode.name) != Some("CSS") {
            assert!(start.elapsed() < Duration::from_secs(5), "never parsed");
            thread::sleep(Duration::from_millis(5));
            editor.refresh();
        }
        let string = page.find("\"x\"").unwrap() + 1;
        let language = |editor: &Editor, offset| editor.language_at(editor.buffer(), offset);
        assert_eq!(language(&editor, page.find("hi").unwrap()), "HTML");
        assert_eq!(language(&editor, string), "JavaScript");
        assert_eq!(editor.syntax_at(string + 1), SyntaxContext::String);

        let commented = |editor: &mut Editor, at: &str| {
            let offset = editor
                .buffer()
                .contents(0..editor.buffer().text().len())
                .find(at)
                .unwrap();
            editor.window_buffer_mut().set_point(offset);
            editor.run_command("comment-line");
            editor.refresh();
            let text = editor.buffer().text();
            editor
                .buffer()
                .contents(text.line_range(text.line_of(offset)).unwrap())
        };
        assert_eq!(commented(&mut editor, "a {"), "/* a { color: red; } */");
        assert_eq!(commented(&mut editor, "let s"), "// let s = \"x\";");
        assert_eq!(commented(&mut editor, "<p>"), "<!-- <p>hi</p> -->");
    }

    #[test]
    fn word_kills_take_a_quoted_word_only_where_a_string_starts() {
        static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);
        let source = "f(\"x\", \"a 'b' c\"); // \"y\" \"z\"\n";
        let killed = |at: &str| {
            let mut editor = Editor::new(&[&LanguagesPlugin, &EditingPlugin, &SYNTAX]);
            editor.add_buffer(Buffer::new("main.rs", Text::from(source), RUST));
            let start = Instant::now();
            while editor.syntax_at(source.find('x').unwrap()) != SyntaxContext::String {
                assert!(start.elapsed() < Duration::from_secs(5), "never parsed");
                thread::sleep(Duration::from_millis(5));
                editor.refresh();
            }
            editor.buffer_mut().set_point(source.find(at).unwrap());
            editor.run_command("kill-word");
            editor.buffer().contents(0..editor.buffer().text().len())
        };
        assert_eq!(killed("\"x\""), "f(, \"a 'b' c\"); // \"y\" \"z\"\n");
        assert_eq!(killed("'b'"), "f(\"x\", \"a b' c\"); // \"y\" \"z\"\n");
        assert_eq!(killed("\" \"z"), "f(\"x\", \"a 'b' c\"); // \"y \"z\"\n");
    }

    #[test]
    fn scripts_and_styles_in_html_are_highlighted_as_their_languages() {
        static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);
        let page = "<style>a { color: red; }</style>\n\
                    <script>let a = 1;</script>\n\
                    <script type=\"module\">export const b = 2;</script>\n\
                    <script lang=\"ts\">interface Card { id: number }</script>\n\
                    <script type=\"module\" lang=\"ts\">let d: boolean;</script>\n\
                    <script type=\"application/json\">{\"on\": true}</script>\n\
                    <script type=\"text/template\">let c = 3;</script>\n";
        let mut editor = Editor::new(&[&SYNTAX]);
        editor.add_buffer(Buffer::new("page.html", Text::from(page), "HTML"));
        let faces = |editor: &Editor| -> Vec<(String, Face)> {
            let buffer = editor.window_buffer();
            let spans = editor.decorations(0..buffer.text().len()).concat();
            spans
                .into_iter()
                .map(|span| (buffer.contents(span.range), span.face))
                .collect()
        };
        let wait_for = |editor: &mut Editor, shows: (&str, Face), shown: bool| {
            let start = Instant::now();
            while faces(editor).contains(&(shows.0.into(), shows.1)) != shown {
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "{shows:?}: {:?}",
                    faces(editor)
                );
                thread::sleep(Duration::from_millis(5));
                editor.refresh();
            }
            faces(editor)
        };
        let shown = wait_for(&mut editor, ("true", Face::Constant), true);
        for expected in [
            ("a", Face::Function),
            ("let", Face::Keyword),
            ("export", Face::Keyword),
            ("interface", Face::Keyword),
            ("number", Face::Type),
            ("boolean", Face::Type),
            ("\"on\"", Face::Function),
        ] {
            assert!(
                shown.contains(&(expected.0.into(), expected.1)),
                "{expected:?} in {shown:?}"
            );
        }
        // The template stays text.
        assert!(!shown.contains(&("3".into(), Face::Constant)), "{shown:?}");

        // Its type changed, it is a script; another style shows; a script deleted goes.
        let template = page.find("text/template").unwrap();
        let buffer = editor.window_buffer_mut();
        buffer.delete(template..template + "text/template".len());
        buffer.set_point(template);
        buffer.insert("module");
        wait_for(&mut editor, ("3", Face::Constant), true);
        let end = editor.window_buffer().text().len();
        editor.window_buffer_mut().set_point(end);
        editor
            .window_buffer_mut()
            .insert("<style>late { }</style>\n");
        wait_for(&mut editor, ("late", Face::Function), true);
        let json = page.find("<script type=\"application/json\">").unwrap();
        let json_end = page[json..].find('\n').unwrap() + json;
        editor.window_buffer_mut().delete(json..json_end);
        wait_for(&mut editor, ("true", Face::Constant), false);
    }

    #[test]
    fn attributes_and_templates_are_highlighted_as_their_languages() {
        static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);
        let page = "<p style=\"color: #fff; margin: 0 auto\" onclick=\"n = go(1)\">x</p>\n\
                    <script>/re/.test(s);\n\
                    const sheet = css`article { width: 2em; }`;\n\
                    const card = html`<b style=\"top: 7px;\">y</b>`;</script>\n";
        let mut editor = Editor::new(&[&SYNTAX]);
        editor.add_buffer(Buffer::new("page.html", Text::from(page), "HTML"));
        let faces = |editor: &Editor| -> Vec<(String, Face)> {
            let buffer = editor.window_buffer();
            let spans = editor.decorations(0..buffer.text().len()).concat();
            spans
                .into_iter()
                .map(|span| (buffer.contents(span.range), span.face))
                .collect()
        };
        let wait_for = |editor: &mut Editor, shows: (&str, Face)| {
            let start = Instant::now();
            while !faces(editor).contains(&(shows.0.into(), shows.1)) {
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "{shows:?}: {:?}",
                    faces(editor)
                );
                thread::sleep(Duration::from_millis(5));
                editor.refresh();
            }
            faces(editor)
        };
        let expected = [
            // The style attribute's declarations, the last without `;`.
            ("#fff", Face::String),
            ("0", Face::Constant),
            // The handler, apart from the script, which would make `1 /re/` a division.
            ("go", Face::Function),
            ("/re/", Face::String),
            // CSS in a script, and CSS in a style in HTML in a script.
            ("article", Face::Function),
            ("em", Face::Type),
            ("b", Face::Function),
            ("7", Face::Constant),
        ];
        let assert_shown = |shown: &[(String, Face)]| {
            for expected in expected {
                assert!(
                    shown.contains(&(expected.0.into(), expected.1)),
                    "{expected:?} in {shown:?}"
                );
            }
            // The attribute's string face gives way to the code in it.
            assert!(
                !shown.iter().any(|(text, _)| text.contains("margin")),
                "{shown:?}"
            );
        };
        assert_shown(&wait_for(&mut editor, ("7", Face::Constant)));
        let documents: Vec<_> = (editor.embedded(editor.window_buffer()).unwrap().iter())
            .map(|code| code.mode)
            .collect();
        assert_eq!(documents, [JAVASCRIPT]);

        // Pieces apart follow an edit before them, and an edit in them.
        editor.window_buffer_mut().set_point(0);
        editor.window_buffer_mut().insert("<code>z</code>");
        assert_shown(&wait_for(&mut editor, ("code", Face::Function)));
        let go = editor
            .buffer()
            .contents(0..editor.buffer().text().len())
            .find("go(")
            .unwrap();
        editor.window_buffer_mut().delete(go..go + 2);
        editor.window_buffer_mut().set_point(go);
        editor.window_buffer_mut().insert("run");
        wait_for(&mut editor, ("run", Face::Function));
    }

    /// The faces of `source` in `mode` once parsed, merged by face.
    fn highlighted(mode: &'static str, source: &str) -> Vec<(String, Face)> {
        static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);
        let mut editor = Editor::new(&[&SYNTAX]);
        editor.add_buffer(Buffer::new("code", Text::from(source), mode));
        let start = Instant::now();
        loop {
            editor.refresh();
            let buffer = editor.window_buffer();
            let spans = editor.decorations(0..buffer.text().len()).concat();
            if !spans.is_empty() {
                return spans
                    .into_iter()
                    .map(|span| (buffer.contents(span.range), span.face))
                    .collect();
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "{mode} never parsed"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn each_tagged_template_is_code_of_its_own() {
        // Run into the first, the second query would be `… where id = select …`.
        let source = "const a = sql`select id from users where id = ${id}`;\n\
                      const b = sql`select name from cards`;\n";
        // The templates come with the parse that brings the first faces.
        let faces = highlighted(TYPESCRIPT, source);
        let selects = (faces.iter())
            .filter(|shown| **shown == ("select".into(), Face::Keyword))
            .count();
        assert_eq!(selects, 2, "{faces:?}");
    }

    /// A mode, code in it, and texts in that code with the faces they get.
    type Sample = (&'static str, &'static str, &'static [(&'static str, Face)]);

    /// Each language's own queries give the faces of its kinds of code.
    #[test]
    fn each_language_highlights_its_code() {
        use Face::*;
        let cases: &[Sample] = &[
            (
                RUST,
                "/// Doc.\n#[derive(Debug)]\npub struct P<'a> { x: &'a str }\n\
                 const MAX: u8 = 0x1f;\nfn go(p: P) -> Option<bool> { let s = \"a\\n\"; \
                 p.run(); vec![1.5]; Some(true) } // c\n",
                &[
                    ("/// Doc.\n", Docstring),
                    ("#[derive(", Builtin),
                    ("Debug", Type),
                    ("pub", Keyword),
                    ("P", Type),
                    ("'a", Constant),
                    ("str", Type),
                    ("MAX", Constant),
                    ("0x1f", Constant),
                    ("go", Function),
                    ("Option", Type),
                    ("\"a\\n\"", String),
                    ("run", Function),
                    ("vec!", Builtin),
                    ("Some", Type),
                    ("true", Constant),
                    ("// c", Comment),
                ],
            ),
            (
                C,
                "#include <stdio.h>\n#define N 4\nstatic unsigned long f(char c) { \
                 return puts(\"x\") + 'c' + N; } /* c */\n",
                &[
                    ("#include", Builtin),
                    ("<stdio.h>", String),
                    ("N", Constant),
                    ("static", Keyword),
                    ("unsigned long", Type),
                    ("f", Function),
                    ("return", Keyword),
                    ("puts", Function),
                    ("\"x\"", String),
                    ("/* c */", Comment),
                ],
            ),
            (
                CPP,
                "namespace app { class Card final { public: auto id() const -> int; }; \
                 auto Card::id() const -> int { return this->n; } }\n",
                &[
                    ("namespace", Keyword),
                    ("app", Type),
                    ("class", Keyword),
                    ("Card", Type),
                    ("public", Keyword),
                    ("auto", Type),
                    ("id", Function),
                    ("this", Keyword),
                ],
            ),
            (
                JAVASCRIPT,
                "import x from \"y\";\nclass Deck extends Base { #n = 1; draw() { return \
                 this.#n; } }\nconst go = async () => /a+/g.test(`n=${n}`);\n\
                 const el = <Card id=\"c\"><b>hi</b></Card>;\n\
                 /**\n * Draws.\n * @param {number} n TODO: more\n */\n",
                &[
                    ("import", Keyword),
                    ("\"y\"", String),
                    ("Deck", Type),
                    ("Base", Type),
                    ("draw", Function),
                    ("this", Keyword),
                    ("go", Function),
                    // The regex's own syntax over the string.
                    ("/a", String),
                    ("+", Keyword),
                    ("test", Function),
                    // JSDoc tags and TODO over the documentation comment.
                    ("/**\n * Draws.\n * ", Docstring),
                    ("@param", Keyword),
                    ("TODO", Keyword),
                    ("Card", Type),
                    ("id", Builtin),
                    ("b", Function),
                ],
            ),
            (
                TYPESCRIPT,
                "interface Card { id: number; draw(): void }\nenum Suit { Hearts }\n\
                 export type Hand = readonly Card[];\n",
                &[
                    ("interface", Keyword),
                    ("Card", Type),
                    ("number", Type),
                    ("draw", Function),
                    ("enum", Keyword),
                    ("Suit", Type),
                    ("Hearts", Constant),
                    ("readonly", Keyword),
                ],
            ),
            (
                TSX,
                "const a = <Box size={2}>{n as number}</Box>;\n",
                &[
                    ("Box", Type),
                    ("size", Builtin),
                    ("2", Constant),
                    ("number", Type),
                ],
            ),
            (
                HTML,
                "<!DOCTYPE html>\n<a href=\"/x\">&amp;</a><!-- c -->\n",
                &[
                    ("<!DOCTYPE html>", Builtin),
                    ("a", Function),
                    ("href", Builtin),
                    ("\"/x\"", String),
                    ("&amp;", Constant),
                    ("<!-- c -->", Comment),
                ],
            ),
            (
                CSS,
                "@media screen { .card:hover > #top { margin: 2px; color: #fff; \
                 width: calc(1em) !important; } } /* c */\n",
                &[
                    ("@media", Keyword),
                    ("card", Type),
                    ("hover", Builtin),
                    ("top", Type),
                    ("2", Constant),
                    ("px", Type),
                    ("#fff", String),
                    ("calc", Function),
                    ("!important", Keyword),
                    ("/* c */", Comment),
                ],
            ),
            (
                JSON,
                "{\"id\": 7, \"ok\": true, \"no\": null, \"s\": \"a\\n\"} // c\n",
                &[
                    ("\"id\"", Function),
                    ("7", Constant),
                    ("true", Constant),
                    ("null", Constant),
                    ("\"a\\n\"", String),
                    ("// c", Comment),
                ],
            ),
            (
                TOML,
                "[server.tls]\nport = 8080\nname = \"x\"\non = true\nat = 1979-05-27 # c\n",
                &[
                    ("server.tls", Type),
                    ("port", Function),
                    ("8080", Constant),
                    ("\"x\"", String),
                    ("true", Constant),
                    ("1979-05-27", Constant),
                    ("# c", Comment),
                ],
            ),
            (
                YAML,
                "%YAML 1.2\n---\nbase: &b !!map\n  port: 80\n  on: true\n  name: \"x\"\n\
                 copy: *b # c\n",
                &[
                    ("%YAML 1.2", Builtin),
                    ("base", Function),
                    ("&b", Constant),
                    ("!!map", Type),
                    ("80", Constant),
                    ("true", Constant),
                    ("\"x\"", String),
                    ("*b", Constant),
                    ("# c", Comment),
                ],
            ),
            (
                DIFF,
                "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n",
                &[
                    ("@@ -1 +1 @@", Function),
                    ("-old", Removed),
                    ("+new", Added),
                ],
            ),
            (
                MAKE,
                "include rules.mk\nCC := cc\nall: main.o\n\t$(CC) -o $@ $(wildcard *.c) # c\n",
                &[
                    ("include", Keyword),
                    ("CC", Constant),
                    ("all", Function),
                    ("$(CC)", Constant),
                    ("$@", Keyword),
                    ("wildcard", Builtin),
                ],
            ),
            (
                CMAKE,
                "function(build NAME)\n  if(WIN32)\n    message(STATUS \"x ${NAME}\")\n  \
                 endif()\nendfunction() # c\n",
                &[
                    ("function", Keyword),
                    ("build", Function),
                    ("if", Keyword),
                    ("message", Function),
                    ("STATUS", Constant),
                    ("${NAME}", Constant),
                    ("# c", Comment),
                ],
            ),
            (
                XML,
                "<?xml version=\"1.0\"?>\n<!-- c --><list a=\"1\">&amp;<x/></list>\n",
                &[
                    ("<?xml version=\"1.0\"?>", Builtin),
                    ("<!-- c -->", Comment),
                    ("list", Function),
                    ("a", Builtin),
                    ("\"1\"", String),
                    ("&amp;", Constant),
                    ("x", Function),
                ],
            ),
            (
                MARKDOWN,
                "# Title\n\n> quoted\n\n```rust\nlet x = 1;\n```\n\n[a]: https://x.dev\n\n\
                 Run `make` per [the guide](https://x.dev/g).\n\n```js\nconst y = 2;\n```\n",
                &[
                    ("# Title\n", Keyword),
                    ("> quoted\n", Docstring),
                    ("rust", Type),
                    // A fenced block in the language it names.
                    ("let", Keyword),
                    ("1", Constant),
                    ("https://x.dev", Constant),
                    // Inline markup within a paragraph.
                    ("`make`", String),
                    ("the guide", Constant),
                    ("https://x.dev/g", Constant),
                    // A short name READMEs use.
                    ("const", Keyword),
                    ("2", Constant),
                ],
            ),
            (
                BASH,
                "#!/bin/bash\nbuild() { local n=$1; echo \"at $n\" | grep -E '^a' > /dev/null; }\n\
                 if [[ -f x ]]; then build $(date); fi\n",
                &[
                    ("#!/bin/bash", Comment),
                    ("build", Function),
                    ("local", Keyword),
                    ("n", Constant),
                    ("echo", Builtin),
                    ("\"at ", String),
                    ("$n", Constant),
                    ("grep", Function),
                    ("'^a'", String),
                    ("if", Keyword),
                    ("date", Function),
                ],
            ),
            (
                SQL,
                "-- c\nselect count(id), 'x' from cards where n > 2 and ok = true;\n\
                 create table t (id int);\n",
                &[
                    ("-- c", Comment),
                    ("select", Keyword),
                    ("count", Function),
                    ("'x'", String),
                    ("from", Keyword),
                    ("cards", Type),
                    ("2", Constant),
                    ("true", Constant),
                    ("int", Type),
                ],
            ),
            (
                PYTHON,
                "\"\"\"Doc.\"\"\"\nimport os\n@cached\nclass Deck(Base):\n    \"\"\"Cards.\"\"\"\n    \
                 def draw(self, n: int = 2) -> None:\n        return len(os.path.join(\"a\")) \
                 or MAX or True  # c\n",
                &[
                    ("\"\"\"Doc.\"\"\"", Docstring),
                    ("import", Keyword),
                    ("@cached", Builtin),
                    ("Deck", Type),
                    ("Base", Type),
                    ("\"\"\"Cards.\"\"\"", Docstring),
                    ("draw", Function),
                    ("self", Keyword),
                    ("int", Type),
                    ("2", Constant),
                    ("None", Constant),
                    ("len", Builtin),
                    ("join", Function),
                    ("\"a\"", String),
                    ("MAX", Constant),
                    ("True", Constant),
                    ("# c", Comment),
                ],
            ),
            (
                GO,
                "package main\nimport \"fmt\"\nconst Max = iota\ntype Card struct { ID int \
                 `json:\"id\"` }\nfunc (c *Card) Draw() error { fmt.Println(len(\"x\"), nil, \
                 3.5); return nil } // c\n",
                &[
                    ("package", Keyword),
                    ("main", Type),
                    ("\"fmt\"", String),
                    ("Max", Constant),
                    ("iota", Constant),
                    ("Card", Type),
                    ("int", Type),
                    ("`json:\"id\"`", Builtin),
                    ("Draw", Function),
                    ("error", Type),
                    ("Println", Function),
                    ("len", Builtin),
                    ("nil", Constant),
                    ("3.5", Constant),
                    ("// c", Comment),
                ],
            ),
            (
                JAVA,
                "/** Doc. */\n@Service\npublic class Deck extends Base { static final int MAX = 1; \
                 void draw() { this.shuffle(\"x\", null, true); } } // c\n",
                &[
                    ("/** Doc. */", Docstring),
                    ("@Service", Builtin),
                    ("public", Keyword),
                    ("Deck", Type),
                    ("Base", Type),
                    ("int", Type),
                    ("MAX", Constant),
                    ("draw", Function),
                    ("this", Keyword),
                    ("shuffle", Function),
                    ("\"x\"", String),
                    ("null", Constant),
                    ("// c", Comment),
                ],
            ),
            (
                CSHARP,
                "/// Doc.\n[Serializable]\npublic class Deck : Base { const int Max = 1; \
                 public void Draw() { Console.WriteLine($\"n={n}\", null, true); } } // c\n",
                &[
                    ("/// Doc.", Docstring),
                    ("Serializable", Builtin),
                    ("public", Keyword),
                    ("Deck", Type),
                    ("Base", Type),
                    ("int", Type),
                    ("Draw", Function),
                    ("WriteLine", Function),
                    ("$\"n=", String),
                    ("null", Constant),
                    ("true", Constant),
                    ("// c", Comment),
                ],
            ),
            (
                SWIFT,
                "import UIKit\n@MainActor\nstruct Card { let id: Int; func draw() throws -> String { \
                 return \"n=\\(id)\" } }\nlet c = Card(id: 2); print(c.draw(), nil, true) // c\n",
                &[
                    ("import", Keyword),
                    ("@MainActor", Builtin),
                    ("struct", Keyword),
                    ("Card", Type),
                    ("Int", Type),
                    ("draw", Function),
                    ("throws", Keyword),
                    ("\"n=\\(", String),
                    ("2", Constant),
                    ("print", Function),
                    ("nil", Constant),
                    ("true", Constant),
                    ("// c", Comment),
                ],
            ),
            (
                OBJC,
                "#import <Foundation/Foundation.h>\n@interface Deck : NSObject\n- (void)draw;\n@end\n\
                 @implementation Deck\n- (void)draw { [self shuffle:YES]; NSLog(@\"x\"); }\n@end\n",
                &[
                    ("#import", Builtin),
                    ("@interface", Keyword),
                    ("Deck", Type),
                    ("void", Type),
                    ("draw", Function),
                    ("self", Keyword),
                    ("shuffle", Function),
                    ("YES", Constant),
                    ("NSLog", Function),
                    ("@end", Keyword),
                ],
            ),
            (
                DART,
                "/// Doc.\n@override\nclass Deck extends Base { final int max = 2; void draw() { \
                 print('n=$n'); shuffle(null, true); } } // c\n",
                &[
                    ("/// Doc.", Docstring),
                    ("@override", Builtin),
                    ("class", Keyword),
                    ("Deck", Type),
                    ("int", Type),
                    ("2", Constant),
                    ("draw", Function),
                    ("print", Function),
                    ("null", Constant),
                    ("true", Constant),
                    ("// c", Comment),
                ],
            ),
            (
                SCALA,
                "object Main { case class Card(id: Int)\n  def draw(n: Int): String = s\"n=$n\" \
                 + println(null) }\n// c\n",
                &[
                    ("object", Keyword),
                    ("Main", Type),
                    ("Card", Type),
                    ("Int", Type),
                    ("draw", Function),
                    ("println", Function),
                    ("null", Constant),
                    ("// c", Comment),
                ],
            ),
            (
                PHP,
                "<?php\nnamespace App;\nclass Deck extends Base { const MAX = 1; public function \
                 draw(int $n): ?string { return strlen(\"x\") . $this->shuffle(null, true); } } // c\n",
                &[
                    ("<?php", Builtin),
                    ("namespace", Keyword),
                    ("App", Type),
                    ("Deck", Type),
                    ("MAX", Constant),
                    ("draw", Function),
                    ("int", Type),
                    ("$n", Constant),
                    ("strlen", Function),
                    ("\"x\"", String),
                    ("this", Keyword),
                    ("shuffle", Function),
                    ("null", Constant),
                    ("// c", Comment),
                ],
            ),
            (
                RUBY,
                "class Deck < Base\n  MAX = 2\n  def draw(n)\n    puts :card, @n, nil, \
                 /a+/\n  end\nend # c\n",
                &[
                    ("class", Keyword),
                    ("Deck", Type),
                    ("MAX", Constant),
                    ("draw", Function),
                    ("puts", Builtin),
                    (":card", Constant),
                    ("@n", Constant),
                    ("nil", Constant),
                    ("/a+/", String),
                    ("# c", Comment),
                ],
            ),
            (
                LUA,
                "local MOD = {}\nfunction MOD.draw(self, n) print(\"x\", nil, true) end -- c\n",
                &[
                    ("local", Keyword),
                    ("MOD", Constant),
                    ("draw", Function),
                    ("self", Keyword),
                    ("print", Builtin),
                    ("\"x\"", String),
                    ("nil", Constant),
                    ("-- c", Comment),
                ],
            ),
            (
                ZIG,
                "const std = @import(\"std\");\npub fn main() !void { const n: u8 = 2; \
                 std.debug.print(\"x\", .{}); } // c\n",
                &[
                    ("const", Keyword),
                    ("@import", Builtin),
                    ("\"std\"", String),
                    ("main", Function),
                    ("void", Type),
                    ("u8", Type),
                    ("2", Constant),
                    ("print", Function),
                    ("// c", Comment),
                ],
            ),
            (
                HASKELL,
                "module Main where\nimport Data.List\n-- | Doc.\ndraw :: Int -> String\ndraw n = \
                 show n ++ \"x\" -- c\n",
                &[
                    ("module", Keyword),
                    ("Main", Type),
                    ("import", Keyword),
                    ("-- | Doc.", Docstring),
                    ("draw", Function),
                    ("Int", Type),
                    ("String", Type),
                    ("\"x\"", String),
                    ("-- c", Comment),
                ],
            ),
            (
                ELIXIR,
                "defmodule Deck do\n  @moduledoc \"Cards.\"\n  def draw(n) do\n    IO.puts(:ok, \
                 \"n=#{n}\", nil, true)\n  end\nend # c\n",
                &[
                    ("defmodule", Keyword),
                    ("Deck", Type),
                    ("\"Cards.\"", Docstring),
                    ("def", Keyword),
                    ("draw", Function),
                    ("IO", Type),
                    ("puts", Function),
                    (":ok", Constant),
                    ("nil", Constant),
                    ("true", Constant),
                    ("# c", Comment),
                ],
            ),
            (
                NIX,
                "{ pkgs, ... }:\nlet name = \"x-${pkgs.v}\"; in { inherit name; src = ./src; \
                 on = true; map = import ./m.nix; } # c\n",
                &[
                    ("pkgs", Constant),
                    ("let", Keyword),
                    ("name", Function),
                    ("map", Function),
                    ("import", Builtin),
                    ("\"x-", String),
                    ("inherit", Keyword),
                    ("./src", String),
                    ("true", Constant),
                    ("# c", Comment),
                ],
            ),
            (
                KOTLIN,
                "package app\n@JvmStatic\nfun draw(n: Int): String = \"n=$n\" + listOf(1).size \
                 // c\nval on = true\nval MAX = null\n",
                &[
                    ("package", Keyword),
                    ("app", Type),
                    ("@JvmStatic", Builtin),
                    ("fun", Keyword),
                    ("draw", Function),
                    ("Int", Type),
                    ("listOf", Function),
                    ("1", Constant),
                    ("// c", Comment),
                    ("true", Constant),
                    ("MAX", Constant),
                    ("null", Constant),
                ],
            ),
            (
                HCL,
                "resource \"aws_s3_bucket\" \"logs\" {\n  count = 2\n  name = \"x-${var.env}\"\n  \
                 tags = merge(local.tags, { on = true })\n} # c\n",
                &[
                    ("resource", Keyword),
                    ("\"aws_s3_bucket\"", Type),
                    ("count", Function),
                    ("2", Constant),
                    ("\"x-", String),
                    ("var", Keyword),
                    ("merge", Function),
                    ("true", Constant),
                    ("# c", Comment),
                ],
            ),
            (
                GRAPHQL,
                "\"\"\"Doc.\"\"\"\ntype Card implements Node { id: ID! @deprecated }\n\
                 query Deck($n: Int = 2) { cards(first: $n) { id } } # c\n",
                &[
                    ("\"\"\"Doc.\"\"\"", Docstring),
                    ("type", Keyword),
                    ("Card", Type),
                    ("Node", Type),
                    ("id", Function),
                    ("ID", Type),
                    ("@deprecated", Builtin),
                    ("query", Keyword),
                    ("Deck", Function),
                    ("$n", Constant),
                    ("2", Constant),
                    ("# c", Comment),
                ],
            ),
            (
                PROTOBUF,
                "syntax = \"proto3\";\npackage app;\nmessage Card { int32 id = 1; Suit suit = 2; }\n\
                 service Deck { rpc Draw (Card) returns (Card); } // c\n",
                &[
                    ("syntax", Keyword),
                    ("\"proto3\"", String),
                    ("message", Keyword),
                    ("Card", Type),
                    ("int32", Type),
                    ("id", Function),
                    ("1", Constant),
                    ("Suit", Type),
                    ("Draw", Function),
                    ("// c", Comment),
                ],
            ),
            (
                SCSS,
                "$gap: 2px;\n@mixin card($n) { margin: $gap; }\n.card { @include card(1); \
                 &:hover { color: darken(#fff, 5%); } } // c\n",
                &[
                    ("$gap", Constant),
                    ("@mixin", Keyword),
                    ("card", Function),
                    ("2", Constant),
                    ("px", Type),
                    ("@include", Keyword),
                    ("darken", Function),
                    ("#fff", String),
                    ("// c", Comment),
                ],
            ),
            (
                GIT_COMMIT,
                "feat(app): draw cards\n\nBody.\n\nRefs: #12\n# Please enter the commit message\n",
                &[
                    ("feat(", Keyword),
                    ("app", Type),
                    ("): draw cards", Keyword),
                    ("Refs: ", Function),
                    ("#12", String),
                    ("# Please enter the commit message\n", Comment),
                ],
            ),
        ];
        for &(mode, source, expected) in cases {
            let faces = highlighted(mode, source);
            for &(text, face) in expected {
                assert!(
                    faces.contains(&(text.into(), face)),
                    "{mode}: {text:?} as {face:?} in {faces:?}"
                );
            }
        }
    }

    #[test]
    fn components_highlight_their_scripts_styles_and_expressions() {
        use Face::*;
        static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);
        let cases: &[Sample] = &[
            (
                VUE,
                "<template><Card :id=\"n + 1\" @click=\"go\">{{ title }}</Card></template>\n\
                 <script lang=\"ts\">let n: number = 2;</script>\n\
                 <style lang=\"scss\">.a { margin: $gap; }</style>\n",
                &[
                    ("Card", Type),
                    (":", Keyword),
                    ("click", Builtin),
                    ("1", Constant),
                    ("let", Keyword),
                    ("number", Type),
                    ("$gap", Constant),
                ],
            ),
            (
                SVELTE,
                "<script>let n = 2;</script>\n{#if n > 1}<p>{n + 3}</p>{/if}\n\
                 <style>p { margin: 0; }</style>\n",
                &[
                    ("let", Keyword),
                    ("2", Constant),
                    ("#if", Keyword),
                    ("1", Constant),
                    ("p", Function),
                    ("3", Constant),
                    ("0", Constant),
                ],
            ),
        ];
        for &(mode, source, expected) in cases {
            let mut editor = Editor::new(&[&SYNTAX]);
            editor.add_buffer(Buffer::new("component", Text::from(source), mode));
            let faces = |editor: &Editor| -> Vec<(std::string::String, Face)> {
                let buffer = editor.window_buffer();
                let spans = editor.decorations(0..buffer.text().len()).concat();
                spans
                    .into_iter()
                    .map(|span| (buffer.contents(span.range), span.face))
                    .collect()
            };
            let start = Instant::now();
            while !expected
                .iter()
                .all(|&(text, face)| faces(&editor).contains(&(text.into(), face)))
            {
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "{mode}: {expected:?} in {:?}",
                    faces(&editor)
                );
                thread::sleep(Duration::from_millis(5));
                editor.refresh();
            }
            // Their scripts are their languages for editing, but no server's documents.
            let embedded = editor.embedded(editor.window_buffer()).unwrap();
            assert!(!embedded.is_empty(), "{mode}");
            assert!(embedded.iter().all(|code| !code.servers), "{mode}");
        }
    }

    #[test]
    fn code_painted_over_stays_its_host_language_for_editing() {
        static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);
        let source = "/**\n * @param n\n */\nlet a = 1;\n";
        let mut editor = Editor::new(&[&SYNTAX]);
        editor.add_buffer(Buffer::new("a.js", Text::from(source), JAVASCRIPT));
        let param = source.find("param").unwrap();
        let start = Instant::now();
        while !editor
            .decorations(param..param + 5)
            .concat()
            .iter()
            .any(|span| span.face == Face::Keyword)
        {
            assert!(start.elapsed() < Duration::from_secs(5), "never painted");
            thread::sleep(Duration::from_millis(5));
            editor.refresh();
        }
        assert_eq!(editor.syntax_at(param + 1), SyntaxContext::Comment);
        assert!(editor.embedded(editor.buffer()).unwrap().is_empty());
    }

    #[test]
    fn builtins_are_called_not_any_name_alike() {
        let faces = highlighted(RUBY, "items.each { |p| p.save }\np items\n");
        let builtins: Vec<_> = (faces.iter())
            .filter(|(_, face)| *face == Face::Builtin)
            .collect();
        assert_eq!(builtins, [&("p".into(), Face::Builtin)], "{faces:?}");
    }

    #[test]
    fn interpolations_are_code_rather_than_string() {
        let cases = [
            (JAVASCRIPT, "let s = `hi ${name}!`;\n", ["`hi ", "!`"]),
            (TYPESCRIPT, "let s = `hi ${name}!`;\n", ["`hi ", "!`"]),
            (PYTHON, "s = f\"hi {name}!\"\n", ["f\"hi ", "!\""]),
            (RUBY, "s = \"hi #{name}!\"\n", ["\"hi ", "!\""]),
            (
                RUST,
                "fn f() { println!(\"hi {name}!\"); }\n",
                ["\"hi ", "!\""],
            ),
        ];
        for (mode, source, strings) in cases {
            let faces = highlighted(mode, source);
            let shown: Vec<_> = (faces.iter())
                .filter(|(_, face)| *face == Face::String)
                .map(|(text, _)| text.as_str())
                .collect();
            assert_eq!(shown, strings, "{mode}: {faces:?}");
        }
    }

    #[test]
    fn each_extension_and_file_name_belongs_to_one_mode() {
        let mut seen = HashSet::new();
        for mode in MODES.iter() {
            for claimed in mode.extensions.iter().chain(mode.file_names) {
                assert!(seen.insert(claimed), "{claimed}");
            }
        }
    }

    #[test]
    fn modes_are_chosen_by_file_name_before_extension() {
        let mut modes = Modes::default();
        for &mode in MODES.iter() {
            modes.add(mode);
        }
        let mode_of = |path: &str| modes.name_for(Some(Path::new(path)));
        assert_eq!(mode_of("src/Makefile"), "Make");
        assert_eq!(mode_of("CMakeLists.txt"), "CMake");
        assert_eq!(mode_of("notes.txt"), "Fundamental");
        assert_eq!(mode_of("/Users/me/.zshrc"), "Bash");
        assert_eq!(mode_of("tsconfig.json"), "JSON");

        // A file's own word comes first, in the names Emacs and vim give the modes.
        let mode_of_file =
            |path: &str, text: &str| modes.name_for_file(Path::new(path), &Text::from(text));
        assert_eq!(
            mode_of_file("bin/serve", "#!/usr/bin/env node\n"),
            "JavaScript"
        );
        assert_eq!(mode_of_file("bin/build", "#!/bin/zsh\n"), "Bash");
        assert_eq!(mode_of_file("bin/check", "#!/usr/bin/python3\n"), "Python");
        assert_eq!(mode_of_file("lib.h", "// -*- mode: c++ -*-\n"), "C++");
        assert_eq!(mode_of_file("env", "# -*- sh -*-\n"), "Bash");
        assert_eq!(
            mode_of_file("conf.in", "x\n// vim: set ft=javascriptreact :\n"),
            "JSX"
        );
        assert_eq!(mode_of_file("index", "<!DOCTYPE html>\n<html>\n"), "HTML");
        assert_eq!(mode_of_file("feed", "<?xml version=\"1.0\"?>\n"), "XML");
    }

    #[test]
    fn servers_serve_known_modes() {
        let servers = remacs_lsp::catalog_servers(SERVERS).unwrap();
        for server in &servers {
            for (mode, _) in server.languages {
                let known = MODES.iter().any(|known| known.name == *mode);
                assert!(known, "{}: {mode}", server.name);
            }
        }
    }

    #[test]
    fn every_mode_with_a_language_server_has_one_built_in() {
        let servers = remacs_lsp::catalog_servers(SERVERS).unwrap();
        // Make, diff and commit messages have no language server worth running; the rest
        // came with their grammars (#102) and get theirs later.
        let without = [
            "Make",
            "Diff",
            "Git Commit",
            "Kotlin",
            "HCL",
            "GraphQL",
            "Protobuf",
            "Vue",
            "Svelte",
        ];
        for mode in MODES.iter().filter(|mode| !without.contains(&mode.name)) {
            let served = servers
                .iter()
                .any(|server| server.languages.iter().any(|(name, _)| *name == mode.name));
            assert!(served, "{}", mode.name);
        }
    }
}
