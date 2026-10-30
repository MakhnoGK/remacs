use std::ops::Range;
use std::path::{Path, PathBuf};

use remacs_text::Text;

use super::Editor;
use crate::buffer::Buffer;
use crate::decoration::Span;
use crate::mode::Mode;
use crate::popup::CodeHighlighter;
use crate::syntax::{self, Block, Embedded, Indentation, SyntaxContext};

impl Editor {
    pub fn mode_for(&self, path: Option<&Path>) -> &'static str {
        self.registry.modes.name_for(path)
    }

    /// The mode of the file at `path` holding `text`: a modeline or `#!` line in it
    /// first, as Emacs `set-auto-mode`.
    pub fn mode_for_file(&self, path: &Path, text: &Text) -> &'static str {
        self.registry.modes.name_for_file(path, text)
    }

    /// The mode named `name`; none for Fundamental and unknown names.
    pub fn mode(&self, name: &str) -> Option<&Mode> {
        self.registry.modes.get(name)
    }

    /// The code of other languages in `buffer`, like CSS in HTML's `<style>`; `None` while
    /// it cannot be told, e.g. before the buffer is parsed again after an edit. Empty
    /// without a plugin that tells it.
    pub fn embedded(&self, buffer: &Buffer) -> Option<Vec<Embedded>> {
        if self.registry.embedded_probes.is_empty() {
            return Some(Vec::new());
        }
        self.registry.embedded_probes.iter().find_map(|probe| probe(buffer))
    }

    /// The document servers and formatters are given for `file`'s code in `mode`, named
    /// with the mode's first extension.
    pub fn embedded_document(&self, file: &Path, mode: &'static str) -> PathBuf {
        let extension = self.mode(mode).and_then(|mode| mode.extensions.first().copied());
        syntax::embedded_document(file, extension.unwrap_or(mode))
    }

    /// The mode of the language at `offset` in `buffer`: code of another language there,
    /// else the buffer's own.
    pub fn language_at(&self, buffer: &Buffer, offset: usize) -> &'static str {
        let embedded = self.embedded(buffer).unwrap_or_default();
        let code = embedded.into_iter().find(|code| code.holds(offset));
        code.map_or(buffer.mode(), |code| code.mode)
    }

    /// The major mode of the language at `offset` in the current buffer, as
    /// [`Self::language_at`] tells it; none for Fundamental.
    pub fn mode_at(&self, offset: usize) -> Option<&Mode> {
        self.registry.modes.get(self.language_at(self.buffer(), offset))
    }

    /// Where `line` of `buffer` takes its indentation from, as its syntax tells it; `None`
    /// when no plugin can tell.
    pub fn indentation_of(&self, buffer: &Buffer, line: usize) -> Option<Indentation> {
        self.registry.indent_probes.iter().find_map(|probe| probe(buffer, line))
    }

    /// The blocks of `buffer`'s code that touch `lines`, which folds can hide; `None`
    /// when no plugin can tell.
    pub fn blocks(&self, buffer: &Buffer, lines: Range<usize>) -> Option<Vec<Block>> {
        self.registry.block_probes.iter().find_map(|probe| probe(buffer, lines.clone()))
    }

    /// Whether `offset` in the current buffer is in a string, a comment or code; code
    /// when no plugin can tell.
    pub fn syntax_at(&self, offset: usize) -> SyntaxContext {
        let buffer = self.buffer();
        let context = self.registry.syntax_probes.iter().find_map(|probe| probe(buffer, offset));
        context.unwrap_or(SyntaxContext::Code)
    }

    /// Faces for `code` written in `mode`, sorted and non-overlapping; none when no
    /// highlighter knows the mode.
    pub fn highlight_code(&self, mode: &'static str, code: &str) -> Vec<Span> {
        let highlighters = &self.registry.code_highlighters;
        highlighters.iter().find_map(|highlight| highlight(mode, code)).unwrap_or_default()
    }

    /// What [`Editor::highlight_code`] asks, for a decorator, which has only the buffer.
    pub fn code_highlighters(&self) -> Vec<CodeHighlighter> {
        self.registry.code_highlighters.clone()
    }
}
