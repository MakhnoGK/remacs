//! Built-in plugin: major modes, chosen by file name or extension, their tree-sitter
//! grammars and language servers.

mod grammars;
mod injections;
mod modes;
mod servers;

use remacs_core::{Plugin, Registry};

pub use grammars::GRAMMARS;
pub use injections::INJECTIONS;
pub use modes::PRETTIER_MODES;
pub use servers::SERVERS;

pub struct LanguagesPlugin;

impl Plugin for LanguagesPlugin {
    fn register(&self, registry: &mut Registry) {
        for &mode in modes::MODES {
            registry.modes.add(mode);
        }
    }
}

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
    use crate::modes::MODES;

    #[test]
    fn every_mode_has_a_grammar_whose_queries_compile() {
        for &Mode { name: mode, .. } in MODES {
            let grammar = GRAMMARS
                .iter()
                .find(|grammar| grammar.mode == mode)
                .unwrap();
            let language = Language::new(grammar.language);
            Query::new(&language, &grammar.highlights.concat())
                .unwrap_or_else(|error| panic!("{mode}: {error}"));
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
    fn scripts_and_styles_in_html_are_highlighted_as_their_languages() {
        static SYNTAX: SyntaxPlugin = SyntaxPlugin::new(GRAMMARS, INJECTIONS);
        let page = "<style>a { color: red; }</style>\n\
                    <script>let a = 1;</script>\n\
                    <script type=\"module\">export const b = 2;</script>\n\
                    <script lang=\"ts\">interface Card { id: number }</script>\n\
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
            ("\"on\"", Face::String),
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
    fn each_extension_and_file_name_belongs_to_one_mode() {
        let mut seen = HashSet::new();
        for mode in MODES {
            for claimed in mode.extensions.iter().chain(mode.file_names) {
                assert!(seen.insert(claimed), "{claimed}");
            }
        }
    }

    #[test]
    fn modes_are_chosen_by_file_name_before_extension() {
        let mut modes = Modes::default();
        for &mode in MODES {
            modes.add(mode);
        }
        let mode_of = |path: &str| modes.name_for(Some(Path::new(path)));
        assert_eq!(mode_of("src/Makefile"), "Make");
        assert_eq!(mode_of("CMakeLists.txt"), "CMake");
        assert_eq!(mode_of("notes.txt"), "Fundamental");
        assert_eq!(mode_of("/Users/me/.zshrc"), "Bash");
        assert_eq!(mode_of("tsconfig.json"), "JSON");
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
        // Make and diff have no language server worth running.
        let without = ["Make", "Diff"];
        for mode in MODES.iter().filter(|mode| !without.contains(&mode.name)) {
            let served = servers
                .iter()
                .any(|server| server.languages.iter().any(|(name, _)| *name == mode.name));
            assert!(served, "{}", mode.name);
        }
    }
}
