use std::ops::Range;
use std::rc::Rc;

use remacs_text::Text;

use crate::buffer::Buffer;
use crate::command::{CommandError, CommandResult};
use crate::completion::{Candidate, Completions};
use crate::editor::Editor;
use crate::mode::FUNDAMENTAL_MODE;

/// Rows shown at once, like `vertico-count`.
pub const VISIBLE_CANDIDATES: usize = 10;

const QUESTION_HINTS: &[(&str, &str)] = &[("y", "Yes"), ("n", "No"), ("C-g", "Cancel")];

/// What the counter next to the input shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Count {
    /// `3/7`
    #[default]
    Position,
    /// `6 matches`
    Matches,
    /// `8`
    Total,
}

/// How a minibuffer prompt looks and behaves; the command that reads fills it in.
#[derive(Default)]
pub struct Prompt {
    pub label: String,
    /// Emacs completion category (`command`, `file`, `buffer`), for commands that act on
    /// the selected candidate.
    pub category: &'static str,
    /// Dim text before the input, such as the directory the input is relative to.
    pub prefix: String,
    pub placeholder: &'static str,
    pub initial_input: String,
    /// Selected first, and the answer when the input is empty.
    pub default: Option<String>,
    pub require_match: bool,
    pub count: Count,
    pub wide: bool,
    /// Keys and what they do, shown under the candidates.
    pub hints: &'static [(&'static str, &'static str)],
}

pub(crate) type CompletionSource = Rc<dyn Fn(&Editor, &str) -> Completions>;
pub(crate) type AnswerHandler = Box<dyn FnOnce(&mut Editor, String) -> CommandResult>;

/// A prompt being answered: its input is an ordinary buffer, so editing commands work in it.
pub struct Minibuffer {
    prompt: Prompt,
    pub(crate) input: Buffer,
    /// `None` for a y-or-n question.
    pub(crate) source: Option<CompletionSource>,
    pub(crate) on_answer: AnswerHandler,
    completions: Completions,
    selected: usize,
    first_visible: usize,
    /// The input `completions` were computed for.
    completed_input: Option<String>,
}

impl Minibuffer {
    pub(crate) fn new(
        prompt: Prompt,
        source: Option<CompletionSource>,
        on_answer: AnswerHandler,
    ) -> Self {
        let mut input = Buffer::new(
            " *Minibuf*",
            Text::from(prompt.initial_input.as_str()),
            FUNDAMENTAL_MODE,
        );
        input.set_point(prompt.initial_input.len());
        Self {
            prompt,
            input,
            source,
            on_answer,
            completions: Completions::default(),
            selected: 0,
            first_visible: 0,
            completed_input: None,
        }
    }

    pub(crate) fn question(question: String, on_answer: AnswerHandler) -> Self {
        let prompt = Prompt {
            label: format!("{question} (y or n)"),
            hints: QUESTION_HINTS,
            ..Prompt::default()
        };
        Self::new(prompt, None, on_answer)
    }

    pub fn prompt(&self) -> &Prompt {
        &self.prompt
    }

    pub fn input(&self) -> &Buffer {
        &self.input
    }

    pub fn input_text(&self) -> String {
        self.input.contents(0..self.input.text().len())
    }

    pub fn is_question(&self) -> bool {
        self.source.is_none()
    }

    pub fn candidates(&self) -> &[Candidate] {
        &self.completions.candidates
    }

    /// Matches in total, including any beyond `candidates`.
    pub fn total(&self) -> usize {
        self.completions.total
    }

    pub fn selected(&self) -> Option<usize> {
        (self.selected < self.candidates().len()).then_some(self.selected)
    }

    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.candidates().get(self.selected)
    }

    /// Indices of the candidates on screen; the selection is always among them.
    pub fn visible(&self) -> Range<usize> {
        self.first_visible..(self.first_visible + VISIBLE_CANDIDATES).min(self.candidates().len())
    }

    pub fn select(&mut self, index: usize) {
        self.selected = index.min(self.candidates().len().saturating_sub(1));
        if self.selected < self.first_visible {
            self.first_visible = self.selected;
        } else if self.selected >= self.first_visible + VISIBLE_CANDIDATES {
            self.first_visible = self.selected + 1 - VISIBLE_CANDIDATES;
        }
    }

    pub fn select_next(&mut self) {
        self.select(self.selected + 1);
    }

    pub fn select_previous(&mut self) {
        self.select(self.selected.saturating_sub(1));
    }

    /// Replaces the input with the selected candidate, like TAB in vertico.
    pub fn complete(&mut self) -> CommandResult {
        let value = self.selected_candidate().ok_or("No match")?.value.clone();
        let len = self.input.text().len();
        self.input.delete(0..len);
        self.input.insert(&value);
        Ok(())
    }

    pub(crate) fn needs_completions(&self) -> bool {
        !self.is_question() && self.completed_input.as_deref() != Some(&self.input_text())
    }

    pub(crate) fn set_completions(&mut self, completions: Completions, input: String) {
        let input_changed = self.completed_input.as_deref() != Some(&input);
        self.completions = completions;
        if input_changed {
            let default = self.prompt.default.as_ref().filter(|_| input.is_empty());
            self.selected = default
                .and_then(|default| {
                    self.candidates()
                        .iter()
                        .position(|candidate| &candidate.value == default)
                })
                .unwrap_or(0);
            self.first_visible = 0;
        }
        self.completed_input = Some(input);
        self.select(self.selected);
    }

    /// The selected candidate, else the input, else the default.
    pub(crate) fn answer(&self, literal_input: bool) -> Result<String, CommandError> {
        let input = self.input_text();
        if !literal_input && let Some(candidate) = self.selected_candidate() {
            return Ok(candidate.value.clone());
        }
        if input.is_empty()
            && let Some(default) = &self.prompt.default
        {
            return Ok(default.clone());
        }
        if self.prompt.require_match && !literal_input {
            return Err("No match".into());
        }
        Ok(input)
    }
}
