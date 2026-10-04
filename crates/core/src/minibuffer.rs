use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use remacs_text::Text;

use crate::buffer::Buffer;
use crate::command::{CommandError, CommandResult};
use crate::completion::{Candidate, Completions, Selection};
use crate::editor::Editor;
use crate::mode::FUNDAMENTAL_MODE;
use crate::table::Mark;

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

/// A y-or-n question drawn as a dialog that lists what it is about, like the window Emacs
/// dired pops up before deleting files.
pub struct Dialog {
    pub title: String,
    /// What answering yes does.
    pub explanation: String,
    pub items: Vec<DialogItem>,
    pub option: Option<DialogOption>,
    pub yes_label: String,
    pub no_label: &'static str,
    /// Yes destroys something; drawn as a warning.
    pub destructive: bool,
}

pub struct DialogItem {
    pub mark: Option<Mark>,
    pub label: String,
    /// E.g. a size.
    pub detail: String,
}

/// A setting the answer depends on, switched by `key` or a click before answering.
pub struct DialogOption {
    pub key: char,
    pub label: &'static str,
    pub on: bool,
}

/// Draws a dialog with its option on or off.
pub(crate) type DialogDescription = Rc<dyn Fn(bool) -> Dialog>;

pub(crate) type CompletionSource = Rc<dyn Fn(&Editor, &str) -> Completions>;
pub(crate) type AnswerHandler = Box<dyn FnOnce(&mut Editor, String) -> CommandResult>;

/// A prompt being answered: its input is an ordinary buffer, so editing commands work in it.
pub struct Minibuffer {
    prompt: Prompt,
    pub(crate) input: Buffer,
    /// `None` for a y-or-n question.
    pub(crate) source: Option<CompletionSource>,
    pub(crate) on_answer: AnswerHandler,
    /// For a question drawn as a dialog, with its option's current state.
    dialog: Option<(DialogDescription, Rc<Cell<bool>>)>,
    completions: Completions,
    selection: Selection,
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
            dialog: None,
            completions: Completions::default(),
            selection: Selection::default(),
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

    pub(crate) fn dialog_question(
        describe: DialogDescription,
        option: Rc<Cell<bool>>,
        on_answer: AnswerHandler,
    ) -> Self {
        let mut minibuffer = Self::question(describe(option.get()).title, on_answer);
        minibuffer.dialog = Some((describe, option));
        minibuffer
    }

    /// How a question drawn as a dialog looks now.
    pub fn dialog(&self) -> Option<Dialog> {
        let (describe, option) = self.dialog.as_ref()?;
        Some(describe(option.get()))
    }

    /// Switches the dialog's option when `key` is its key, or for a click, `None`.
    pub(crate) fn toggle_option(&self, key: Option<char>) -> bool {
        let Some((describe, option)) = &self.dialog else {
            return false;
        };
        let matches = describe(option.get())
            .option
            .is_some_and(|dialog_option| key.is_none_or(|key| key == dialog_option.key));
        if matches {
            option.set(!option.get());
        }
        matches
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
        let selected = self.selection.selected();
        (selected < self.candidates().len()).then_some(selected)
    }

    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.candidates().get(self.selection.selected())
    }

    /// Indices of the candidates on screen; the selection is always among them.
    pub fn visible(&self) -> Range<usize> {
        self.selection
            .visible(self.candidates().len(), VISIBLE_CANDIDATES)
    }

    pub fn select(&mut self, index: usize) {
        let len = self.candidates().len();
        self.selection.select(index, len, VISIBLE_CANDIDATES);
    }

    pub fn select_next(&mut self) {
        self.select(self.selection.selected() + 1);
    }

    pub fn select_previous(&mut self) {
        self.select(self.selection.selected().saturating_sub(1));
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
        let index = if input_changed {
            self.selection = Selection::default();
            let default = self.prompt.default.as_ref().filter(|_| input.is_empty());
            default
                .and_then(|default| {
                    self.candidates()
                        .iter()
                        .position(|candidate| &candidate.value == default)
                })
                .unwrap_or(0)
        } else {
            self.selection.selected()
        };
        self.completed_input = Some(input);
        self.select(index);
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
