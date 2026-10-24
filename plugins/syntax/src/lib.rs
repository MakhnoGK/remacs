//! Built-in plugin: tree-sitter parsing on a background thread and syntax highlighting.

mod buffer_syntax;
mod downloads;
mod fallback;
mod highlighter;
mod indent;
mod locals;
mod matching_pair;
mod parser_thread;
mod queries;
mod sexp;
mod sexp_edit;
mod snippet;
mod todo;
mod wasm;

use std::cell::RefCell;
use std::rc::Rc;

pub use highlighter::{face_for, known_capture};
pub use queries::keep_queries_in;
use remacs_core::{Command, Hook, MenuItem, Plugin, Registry, Section};
use remacs_tools::ToolKind;
use serde::Deserialize;
use tree_sitter::Language;
pub use tree_sitter_language::LanguageFn;
pub use wasm::load as load_downloaded;

/// `syntax.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct SyntaxSettings {
    /// Whether the bracket or tag at point is highlighted with the one it pairs with.
    pub matching_pairs: bool,
    /// Whether code of other languages in a file, like CSS in HTML's `<style>`, is parsed
    /// as its own language, for its highlighting, comments, pairs, servers and formatting.
    pub embedded_languages: bool,
    /// Words highlighted in comments, like Emacs `hl-todo`.
    pub todo_keywords: Vec<String>,
    /// Whether a function's parameters and local variables are highlighted wherever they
    /// are used in it.
    pub local_variables: bool,
    /// How much is highlighted, 1–4, like Emacs `treesit-font-lock-level`.
    pub highlight_level: u8,
    /// Whether a grammar remacs does not build in is downloaded when a file wants it.
    pub download_grammars: bool,
}

impl Default for SyntaxSettings {
    fn default() -> Self {
        Self {
            matching_pairs: true,
            embedded_languages: true,
            local_variables: true,
            highlight_level: 4,
            download_grammars: true,
            todo_keywords: ["TODO", "FIXME", "XXX", "HACK", "BUG", "NOTE"]
                .map(String::from)
                .into(),
        }
    }
}

impl Section for SyntaxSettings {
    const NAME: &str = "syntax";
}

/// A tree-sitter grammar and its highlight queries, for one major mode.
pub struct Grammar {
    pub mode: &'static str,
    pub language: Source,
    /// Queries joined in order; when several patterns capture one node, the last wins.
    pub highlights: &'static [&'static str],
}

/// Where a grammar's parser comes from.
#[derive(Clone, Copy)]
pub enum Source {
    /// Compiled into remacs.
    Built(LanguageFn),
    /// Built to WebAssembly by remacs's CI and downloaded once a file wants it (ADR 0087):
    /// `tree-sitter-<name>.wasm` of the `grammars-<release>` release, whose node types
    /// remacs's queries are written for.
    Downloaded {
        name: &'static str,
        release: &'static str,
    },
}

impl Grammar {
    /// Its language; none for a downloaded grammar not loaded yet.
    pub fn language(&self) -> Option<Language> {
        match self.language {
            Source::Built(language) => Some(Language::new(language)),
            Source::Downloaded { name, .. } => wasm::loaded(name),
        }
    }
}

/// Where a mode's text holds code of other languages, e.g. scripts in HTML: each
/// `@injection.content` capture of `query` is parsed with the grammar of the mode its
/// pattern's `#set! injection.language` names, else its `@injection.language` capture's
/// text, ignoring case; that code's own injections are followed in turn. Each match makes
/// a tree, but with `#set! injection.combined` all the ranges of one language do; with
/// `#set! injection.terminator ";"` too, those of such patterns make one of their own:
/// their first and last bytes, like an attribute's quotes, are read as spaces, or the
/// last as `;` where the code lacks it, so that each piece ends before the next. With
/// `#set! injection.overlay` the code is painted over the host's faces, only where it has
/// faces of its own, and stays the host's for editing: TODO in a comment, a regex.
pub struct Injections {
    pub host: &'static str,
    pub query: &'static str,
    /// Whether its combined code without a terminator is a document of its own for servers
    /// and formatters, like a page's scripts; code in strings, like ``css`…` ``, is not.
    pub documents: bool,
    /// Whether language servers are shown those documents, which they can take for wrong
    /// outside their file: a Vue component's script uses names only Vue knows.
    pub servers: bool,
}

/// How a mode's lines are indented, as its queries joined in order capture its tree: the
/// lines of an `@indent.begin` node after its first are a level deeper than that one, and
/// those of an `@indent.align` node line up with its start; a line starting with an
/// `@indent.end` or `@indent.branch` node, like `}` or `else`, is at its begin node's
/// level; a blank line after one ending with an `@indent.open` token, like `{` or
/// Python's `:`, is a level deeper. The innermost such node above the line decides.
pub struct Indents {
    pub mode: &'static str,
    pub queries: &'static [&'static str],
    /// Only blank lines are told, the one `RET` leaves among them: a language whose
    /// indentation is its structure, like Python's, where `TAB` keeps adding blanks.
    pub only_blank_lines: bool,
}

impl Indents {
    pub const fn new(mode: &'static str, queries: &'static [&'static str]) -> Self {
        Self {
            mode,
            queries,
            only_blank_lines: false,
        }
    }

    pub const fn only_blank_lines(self) -> Self {
        Self {
            only_blank_lines: true,
            ..self
        }
    }
}

/// A mode's locals queries, joined in order: its functions' parameters and local
/// variables, highlighted where they are used (ADR 0086).
pub struct Locals {
    pub mode: &'static str,
    pub queries: &'static [&'static str],
}

pub struct SyntaxPlugin {
    grammars: &'static [Grammar],
    injections: &'static [Injections],
    indents: &'static [Indents],
    locals: &'static [Locals],
}

impl SyntaxPlugin {
    pub const fn new(grammars: &'static [Grammar], injections: &'static [Injections]) -> Self {
        Self {
            grammars,
            injections,
            indents: &[],
            locals: &[],
        }
    }

    pub const fn with_indents(self, indents: &'static [Indents]) -> Self {
        Self { indents, ..self }
    }

    pub const fn with_locals(self, locals: &'static [Locals]) -> Self {
        Self { locals, ..self }
    }
}

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(queries::RELOAD, "Read the highlight queries of one's own in remacs's queries/ again", queries::reload),
    Command::new(sexp::FORWARD_SEXP, "Move over the next expression of the syntax tree", sexp::forward_sexp).each_cursor(),
    Command::new(sexp::BACKWARD_SEXP, "Move back over the previous expression of the syntax tree", sexp::backward_sexp).each_cursor(),
    Command::new(sexp::BACKWARD_UP_LIST, "Move to the opening bracket of the list point is in", sexp::backward_up_list).each_cursor(),
    Command::new(sexp::DOWN_LIST, "Move into the next list, after its opening bracket", sexp::down_list).each_cursor(),
    Command::new(sexp::FORWARD_LIST, "Move over the next list between brackets", sexp::forward_list).each_cursor(),
    Command::new(sexp::BACKWARD_LIST, "Move back over the previous list between brackets", sexp::backward_list).each_cursor(),
    Command::new(sexp_edit::TRANSPOSE_SEXPS, "Swap the expressions before and after point", sexp_edit::transpose_sexps),
    Command::new(sexp_edit::KILL_SEXP, "Kill the next expression", sexp_edit::kill_sexp),
    Command::new(sexp_edit::BACKWARD_KILL_SEXP, "Kill the previous expression", sexp_edit::backward_kill_sexp),
    Command::new(sexp_edit::MARK_SEXP, "Set the mark after the next expression, then after the one after it", sexp_edit::mark_sexp),
    Command::new(sexp_edit::SLURP_FORWARD, "Move the closing bracket of the list point is in over the next expression", sexp_edit::slurp_forward),
    Command::new(sexp_edit::BARF_FORWARD, "Move the closing bracket of the list point is in before its last expression", sexp_edit::barf_forward),
    Command::new(sexp_edit::SLURP_BACKWARD, "Move the opening bracket of the list point is in over the previous expression", sexp_edit::slurp_backward),
    Command::new(sexp_edit::BARF_BACKWARD, "Move the opening bracket of the list point is in after its first expression", sexp_edit::barf_backward),
    Command::new(sexp_edit::SPLICE_SEXP, "Delete the brackets of the list point is in", sexp_edit::splice_sexp),
    Command::new(sexp_edit::RAISE_SEXP, "Replace the list point is in with the expression at point", sexp_edit::raise_sexp),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-M-f", sexp::FORWARD_SEXP),
    ("C-M-b", sexp::BACKWARD_SEXP),
    ("C-M-u", sexp::BACKWARD_UP_LIST),
    ("C-M-d", sexp::DOWN_LIST),
    ("C-M-n", sexp::FORWARD_LIST),
    ("C-M-p", sexp::BACKWARD_LIST),
    ("C-M-t", sexp_edit::TRANSPOSE_SEXPS),
    ("C-M-k", sexp_edit::KILL_SEXP),
    ("C-M-DEL", sexp_edit::BACKWARD_KILL_SEXP),
    ("C-M-SPC", sexp_edit::MARK_SEXP),
    ("C-)", sexp_edit::SLURP_FORWARD),
    ("C-}", sexp_edit::BARF_FORWARD),
    ("C-(", sexp_edit::SLURP_BACKWARD),
    ("C-{", sexp_edit::BARF_BACKWARD),
    ("M-D", sexp_edit::SPLICE_SEXP),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Go", "structure", "Next Expression", sexp::FORWARD_SEXP),
    MenuItem::new("Go", "structure", "Previous Expression", sexp::BACKWARD_SEXP),
    MenuItem::new("Go", "structure", "Out of List", sexp::BACKWARD_UP_LIST),
    MenuItem::new("Go", "structure", "Into Next List", sexp::DOWN_LIST),
    MenuItem::new("Edit", "selection", "Select Expression", sexp_edit::MARK_SEXP),
    MenuItem::new("Edit", "structure", "Kill Expression", sexp_edit::KILL_SEXP),
    MenuItem::new("Edit", "structure", "Transpose Expressions", sexp_edit::TRANSPOSE_SEXPS),
    MenuItem::new("Edit", "structure", "Slurp Next Expression", sexp_edit::SLURP_FORWARD),
    MenuItem::new("Edit", "structure", "Barf Last Expression", sexp_edit::BARF_FORWARD),
    MenuItem::new("Edit", "structure", "Remove Brackets", sexp_edit::SPLICE_SEXP),
    MenuItem::new("Edit", "structure", "Raise Expression", sexp_edit::RAISE_SEXP),
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
        let settings = Rc::new(RefCell::new(SyntaxSettings::default()));
        let tools = downloads::tools(grammars);
        registry.settings_hooks.push(Rc::new({
            let settings = Rc::clone(&settings);
            move |editor| {
                *settings.borrow_mut() = editor.settings_section::<SyntaxSettings>();
                queries::know_grammars(editor, grammars);
                remacs_tools::provide(editor, ToolKind::Grammar, &tools);
            }
        }));
        let sync: Hook = Rc::new({
            let settings = Rc::clone(&settings);
            move |editor| {
                downloads::want(editor, grammars, settings.borrow().download_grammars);
                let injections = if settings.borrow().embedded_languages {
                    injections
                } else {
                    &[]
                };
                buffer_syntax::sync(editor, grammars, injections)
            }
        });
        registry.post_command_hooks.push(Rc::clone(&sync));
        registry.display_hooks.push(sync);
        let locals = self.locals;
        let locals_queries = RefCell::new(locals::LocalsQueries::default());
        registry.decorators.push(Rc::new({
            let settings = Rc::clone(&settings);
            move |buffer, range| {
                let settings = settings.borrow();
                let mut spans =
                    buffer_syntax::highlight(buffer, range.clone(), settings.highlight_level);
                if settings.local_variables {
                    spans = locals_queries
                        .borrow_mut()
                        .paint(locals, buffer, range, spans);
                }
                todo::mark(buffer, spans, &settings.todo_keywords)
            }
        }));
        registry.decorators.push(Rc::new({
            let settings = Rc::clone(&settings);
            move |buffer, range| {
                if settings.borrow().matching_pairs {
                    matching_pair::highlight(buffer, range)
                } else {
                    Vec::new()
                }
            }
        }));
        registry
            .syntax_probes
            .push(Rc::new(buffer_syntax::context_at));
        registry
            .embedded_probes
            .push(Rc::new(buffer_syntax::embedded));
        let indents = self.indents;
        let indenter = Rc::new(RefCell::new(indent::Indenter::new()));
        registry.indent_probes.push(Rc::new({
            let indenter = Rc::clone(&indenter);
            move |buffer, line| indenter.borrow_mut().indentation(indents, buffer, line)
        }));
        registry.block_probes.push(Rc::new({
            let indenter = Rc::clone(&indenter);
            move |buffer, lines| indenter.borrow_mut().blocks(indents, buffer, lines)
        }));
        // A tree parsed for one command's lines is not kept past it, nor its text.
        registry
            .post_command_hooks
            .push(Rc::new(move |_| indenter.borrow_mut().forget_parse()));
        let snippets = RefCell::new(snippet::SnippetHighlighter::new(grammars));
        registry.code_highlighters.push(Rc::new(move |mode, code| {
            let level = settings.borrow().highlight_level;
            snippets.borrow_mut().highlight(mode, code, level)
        }));
    }
}

#[cfg(test)]
mod tests;
