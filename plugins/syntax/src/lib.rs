//! Built-in plugin: tree-sitter parsing on a background thread and syntax highlighting.

mod buffer_syntax;
mod highlighter;
mod matching_pair;
mod parser_thread;
mod sexp;
mod snippet;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use remacs_core::{Command, MenuItem, Plugin, Registry, Section};
use serde::Deserialize;
pub use tree_sitter_language::LanguageFn;

/// `syntax.toml`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct SyntaxSettings {
    /// Whether the bracket or tag at point is highlighted with the one it pairs with.
    pub matching_pairs: bool,
    /// Whether code of other languages in a file, like CSS in HTML's `<style>`, is parsed
    /// as its own language, for its highlighting, comments, pairs, servers and formatting.
    pub embedded_languages: bool,
}

impl Default for SyntaxSettings {
    fn default() -> Self {
        Self {
            matching_pairs: true,
            embedded_languages: true,
        }
    }
}

impl Section for SyntaxSettings {
    const NAME: &str = "syntax";
}

/// A tree-sitter grammar and its highlight queries, for one major mode.
pub struct Grammar {
    pub mode: &'static str,
    pub language: LanguageFn,
    /// Queries joined in order; when several patterns capture one node, the last wins.
    pub highlights: &'static [&'static str],
}

/// Where a host mode's text holds code of other languages, e.g. scripts in HTML: each
/// `@injection.content` capture of `query` is parsed with the grammar whose mode is the
/// pattern's `#set! injection.language`, ignoring case. All the ranges of one language
/// make one tree.
pub struct Injections {
    pub host: &'static str,
    pub query: &'static str,
}

pub struct SyntaxPlugin {
    grammars: &'static [Grammar],
    injections: &'static [Injections],
}

impl SyntaxPlugin {
    pub const fn new(grammars: &'static [Grammar], injections: &'static [Injections]) -> Self {
        Self {
            grammars,
            injections,
        }
    }
}

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(sexp::FORWARD_SEXP, "Move over the next expression of the syntax tree", sexp::forward_sexp),
    Command::new(sexp::BACKWARD_SEXP, "Move back over the previous expression of the syntax tree", sexp::backward_sexp),
    Command::new(sexp::BACKWARD_UP_LIST, "Move to the opening bracket of the list point is in", sexp::backward_up_list),
    Command::new(sexp::DOWN_LIST, "Move into the next list, after its opening bracket", sexp::down_list),
    Command::new(sexp::FORWARD_LIST, "Move over the next list between brackets", sexp::forward_list),
    Command::new(sexp::BACKWARD_LIST, "Move back over the previous list between brackets", sexp::backward_list),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-M-f", sexp::FORWARD_SEXP),
    ("C-M-b", sexp::BACKWARD_SEXP),
    ("C-M-u", sexp::BACKWARD_UP_LIST),
    ("C-M-d", sexp::DOWN_LIST),
    ("C-M-n", sexp::FORWARD_LIST),
    ("C-M-p", sexp::BACKWARD_LIST),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Go", "structure", "Next Expression", sexp::FORWARD_SEXP),
    MenuItem::new("Go", "structure", "Previous Expression", sexp::BACKWARD_SEXP),
    MenuItem::new("Go", "structure", "Out of List", sexp::BACKWARD_UP_LIST),
    MenuItem::new("Go", "structure", "Into Next List", sexp::DOWN_LIST),
];

impl Plugin for SyntaxPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        let (grammars, injections) = (self.grammars, self.injections);
        let settings = Rc::new(Cell::new(SyntaxSettings::default()));
        registry.settings_hooks.push(Rc::new({
            let settings = Rc::clone(&settings);
            move |editor| settings.set(editor.settings_section::<SyntaxSettings>())
        }));
        registry.post_command_hooks.push(Rc::new({
            let settings = Rc::clone(&settings);
            move |editor| {
                let injections = if settings.get().embedded_languages {
                    injections
                } else {
                    &[]
                };
                buffer_syntax::sync(editor, grammars, injections)
            }
        }));
        registry.decorators.push(Rc::new(buffer_syntax::highlight));
        registry.decorators.push(Rc::new(move |buffer, range| {
            if settings.get().matching_pairs {
                matching_pair::highlight(buffer, range)
            } else {
                Vec::new()
            }
        }));
        registry
            .syntax_probes
            .push(Rc::new(buffer_syntax::context_at));
        registry
            .embedded_probes
            .push(Rc::new(buffer_syntax::embedded));
        let snippets = RefCell::new(snippet::SnippetHighlighter::new(grammars));
        registry.code_highlighters.push(Rc::new(move |mode, code| {
            snippets.borrow_mut().highlight(mode, code)
        }));
    }
}

#[cfg(test)]
mod tests;
