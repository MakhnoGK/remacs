use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use remacs_text::Text;

use crate::buffer::Buffer;
use crate::command::{CommandError, CommandResult};
use crate::completion::{Candidate, Completions, Icon, Selection};
use crate::editor::Editor;
use crate::key::Key;
use crate::keymap::Keymap;
use crate::mode::FUNDAMENTAL_MODE;
use crate::outline::BannerPart;
use crate::table::Mark;
use crate::vc::{FileState, LineCounts};

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
    /// Answers are left out of the history, which is saved across sessions, e.g. text
    /// from the kill ring.
    pub private: bool,
    /// What the answer is resolved against or run in, shown on a line by the input, never
    /// in it.
    pub directory: String,
    pub placeholder: &'static str,
    pub initial_input: String,
    /// Selected first, and the answer when the input is empty.
    pub default: Option<String>,
    pub require_match: bool,
    pub count: Count,
    pub wide: bool,
    /// Keys and what they do, shown under the candidates.
    pub hints: &'static [(&'static str, &'static str)],
    /// Settings the answer depends on; a switch goes above the input, a chip under the
    /// lists.
    pub options: Vec<PromptOption>,
    /// What the prompt acts on, beside the switch, or after the chips when there is no
    /// switch, e.g. `2 marked files`.
    pub subject: String,
    /// The input is the subject of a message with a description under it, like a commit
    /// message; the answer is both, a blank line between them.
    pub message: Option<Message>,
    /// Values the answer depends on, picked in turn by a key or a click; chips before the
    /// options'.
    pub choices: Vec<PromptChoice>,
    /// Listed for reference under the candidates, not chosen from, e.g. the files a
    /// command acts on.
    pub items: Vec<ListItem>,
    pub items_title: &'static str,
    /// Right of the title, e.g. a count and a total size.
    pub items_summary: String,
    /// Says why the answer cannot be taken yet; the prompt then stays open, its input
    /// kept.
    pub check: Option<AnswerCheck>,
    /// Active over every other keymap while the prompt is open, e.g. keys that act on
    /// the selected candidate.
    pub keymap: Option<Keymap>,
    /// The input is not typed: it shows a value the prompt's keys change, and printable
    /// keys are free for its keymap.
    pub read_only: bool,
    /// In place of the field, for a read-only prompt: what it does and a note on the
    /// right, e.g. `Push main` and `2 commits`.
    pub title: Option<(String, String)>,
    /// Under the title, e.g. how far two branches are apart.
    pub summary: Vec<BannerPart>,
    /// Opened from the toolbar: drawn under its right end rather than centered.
    pub from_toolbar: bool,
    /// The chips go under the field, above the candidates, e.g. settings of what the
    /// input makes.
    pub chips_first: bool,
    /// Makes the candidates a list of their own that `minibuffer-next-field` moves to:
    /// there, printable keys and RET look in this keymap, and the input is not typed. The
    /// answer is always the input.
    pub list_keymap: Option<Keymap>,
    /// In place of `hints` while keys go to the list.
    pub list_hints: &'static [(&'static str, &'static str)],
}

pub type AnswerCheck = Rc<dyn Fn(&str) -> Result<(), String>>;

/// How a prompt's message is written.
#[derive(Default)]
pub struct Message {
    /// Characters the subject should keep within, counted beside it.
    pub subject_limit: usize,
    /// In the empty description.
    pub placeholder: &'static str,
    /// Earlier messages, newest first, brought back by `M-p` and `M-n`.
    pub earlier: Vec<String>,
}

/// One of several values a prompt's answer depends on, e.g. the branch a new one starts
/// from.
pub struct PromptChoice {
    pub label: &'static str,
    /// Emacs notation, e.g. `TAB`; it takes the next value.
    pub key: &'static str,
    pub values: Vec<String>,
    /// Shared with the code that reads the prompt.
    pub selected: Rc<Cell<usize>>,
    /// Drawn as segments, all values shown, under the summary; else as a chip.
    pub segmented: bool,
}

impl PromptChoice {
    pub fn value(&self) -> Option<&str> {
        self.values.get(self.selected.get()).map(String::as_str)
    }

    fn next(&self) {
        if !self.values.is_empty() {
            self.selected
                .set((self.selected.get() + 1) % self.values.len());
        }
    }
}

/// A setting a prompt's answer depends on, switched by `M-` and `key`, or a click.
pub struct PromptOption {
    pub key: char,
    /// Off, then on: two labels make a switch between them, one a chip lit while on.
    pub labels: Vec<String>,
    /// Shared with the code that reads the prompt, which sees it as it is.
    pub state: Rc<Cell<bool>>,
    /// Shown, and switched, only while this other state is on, e.g. a setting of moving
    /// while the prompt moves rather than copies.
    pub shown_while: Option<Rc<Cell<bool>>>,
    /// Hidden, and not switched, while this other state is on.
    pub hidden_while: Option<Rc<Cell<bool>>>,
}

impl PromptOption {
    /// A chip lit while `state` is on, always shown.
    pub fn chip(key: char, label: &str, state: &Rc<Cell<bool>>) -> Self {
        Self {
            key,
            labels: vec![label.into()],
            state: Rc::clone(state),
            shown_while: None,
            hidden_while: None,
        }
    }

    pub fn is_switch(&self) -> bool {
        self.labels.len() == 2
    }

    pub fn is_on(&self) -> bool {
        self.state.get()
    }

    pub fn is_shown(&self) -> bool {
        self.shown_while.as_ref().is_none_or(|state| state.get())
            && !self.hidden_while.as_ref().is_some_and(|state| state.get())
    }
}

/// A y-or-n question drawn as a dialog that lists what it is about, like the window Emacs
/// dired pops up before deleting files.
pub struct Dialog {
    pub title: String,
    /// What answering yes does.
    pub explanation: String,
    pub items: Vec<ListItem>,
    pub option: Option<DialogOption>,
    pub yes_label: String,
    pub no_label: &'static str,
    /// Yes destroys something; drawn as a warning.
    pub destructive: bool,
}

/// A row of a list shown for reference, e.g. a file to be deleted.
#[derive(Clone, Debug)]
pub struct ListItem {
    pub icon: Option<Icon>,
    pub mark: Option<Mark>,
    pub label: String,
    /// E.g. a size, or a file's directory when it has a state.
    pub detail: String,
    /// How a changed file differs, drawn as a letter before it.
    pub state: Option<FileState>,
    pub lines: Option<LineCounts>,
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
    /// A message's lines after its subject, which is the input.
    description: Option<Buffer>,
    /// Commands act on the description rather than the input.
    in_description: bool,
    /// Keys go to the candidates, for a prompt with a list keymap.
    in_list: bool,
    /// Of the prompt's earlier messages, the one brought back.
    earlier_message: Option<usize>,
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
        let (subject, description) = match &prompt.message {
            Some(_) => {
                let (subject, description) = split_message(&prompt.initial_input);
                (
                    subject,
                    Some(input_buffer(" *Minibuf-description*", description)),
                )
            }
            None => (prompt.initial_input.as_str(), None),
        };
        let input = input_buffer(" *Minibuf*", subject);
        Self {
            prompt,
            input,
            description,
            in_description: false,
            in_list: false,
            earlier_message: None,
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

    /// Replaces the input, e.g. with a value a key of the prompt picked.
    pub fn set_input(&mut self, text: &str) {
        replace_text(&mut self.input, text);
    }

    /// A message's description, under its subject.
    pub fn description(&self) -> Option<&Buffer> {
        self.description.as_ref()
    }

    pub fn in_description(&self) -> bool {
        self.in_description
    }

    /// The buffer commands edit: the description or the input.
    pub(crate) fn editing(&self) -> &Buffer {
        match &self.description {
            Some(description) if self.in_description => description,
            _ => &self.input,
        }
    }

    pub(crate) fn editing_mut(&mut self) -> &mut Buffer {
        match &mut self.description {
            Some(description) if self.in_description => description,
            _ => &mut self.input,
        }
    }

    /// Whether keys go to the candidates rather than the input.
    pub fn in_list(&self) -> bool {
        self.in_list
    }

    /// Moves between a message's subject and its description, or between the input and
    /// a list of candidates.
    pub fn next_field(&mut self) -> CommandResult {
        if self.description.is_some() {
            self.in_description = !self.in_description;
            return Ok(());
        }
        if self.prompt.list_keymap.is_none() {
            return Err("No other field".into());
        }
        if !self.in_list && self.candidates().is_empty() {
            return Err("Nothing listed".into());
        }
        self.in_list = !self.in_list;
        Ok(())
    }

    /// Selects candidate `index` and moves the keys to the list, as a click on it does.
    pub fn focus_candidate(&mut self, index: usize) {
        self.select(index);
        self.in_list = self.prompt.list_keymap.is_some();
    }

    /// The keymap active over the others now: the list's while keys go to it.
    pub fn keymap(&self) -> Option<&Keymap> {
        if self.in_list {
            self.prompt.list_keymap.as_ref()
        } else {
            None
        }
    }

    /// Printable keys type into the input.
    pub fn is_typed(&self) -> bool {
        !self.prompt.read_only && !self.in_list
    }

    /// Replaces the message with an earlier one, or a later one, like Emacs
    /// `previous-history-element`.
    pub fn recall_message(&mut self, older: bool) -> CommandResult {
        let earlier = &self.prompt.message.as_ref().ok_or("Not a message")?.earlier;
        let index = match (self.earlier_message, older) {
            (None, true) => 0,
            (None, false) => return Err("No later message".into()),
            (Some(index), true) => index + 1,
            (Some(0), false) => return Err("No later message".into()),
            (Some(index), false) => index - 1,
        };
        let message = earlier.get(index).ok_or("No earlier message")?;
        let (subject, description) = split_message(message);
        replace_text(&mut self.input, subject);
        if let Some(buffer) = &mut self.description {
            replace_text(buffer, description);
        }
        self.earlier_message = Some(index);
        Ok(())
    }

    /// The subject, and the description after a blank line if there is one.
    fn message_text(&self) -> String {
        let subject = self.input_text();
        let description = self
            .description
            .as_ref()
            .map(|buffer| buffer.contents(0..buffer.text().len()))
            .unwrap_or_default();
        let description = description.trim_end();
        if description.is_empty() {
            subject
        } else {
            format!("{subject}\n\n{description}")
        }
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

    /// Said in place of the count, e.g. `New folder`.
    pub fn note(&self) -> Option<&str> {
        self.completions.note.as_deref()
    }

    /// The completions' label if they give one, else the prompt's.
    pub fn label(&self) -> &str {
        self.completions
            .label
            .as_deref()
            .unwrap_or(&self.prompt.label)
    }

    /// The completions' subject if they give one, else the prompt's.
    pub fn subject(&self) -> &str {
        self.completions
            .subject
            .as_deref()
            .unwrap_or(&self.prompt.subject)
    }

    pub fn completions(&self) -> &Completions {
        &self.completions
    }

    /// Keys and what they do now: the list's while keys go to it, else the completions'
    /// if they say, else the prompt's.
    pub fn hints(&self) -> &'static [(&'static str, &'static str)] {
        if self.in_list {
            return self.prompt.list_hints;
        }
        self.completions.hints.unwrap_or(self.prompt.hints)
    }

    /// The shown prompt option `M-key` switches.
    pub(crate) fn option_for_key(&self, key: Key) -> Option<usize> {
        self.prompt.options.iter().position(|option| {
            option.is_shown() && Key::parse(&format!("M-{}", option.key)) == Some(key)
        })
    }

    /// The choice `key` picks the next value of.
    pub(crate) fn choice_for_key(&self, key: Key) -> Option<usize> {
        self.prompt
            .choices
            .iter()
            .position(|choice| Key::parse(choice.key) == Some(key))
    }

    pub(crate) fn next_choice(&self, index: usize) {
        if let Some(choice) = self.prompt.choices.get(index) {
            choice.next();
        }
    }

    pub(crate) fn toggle_prompt_option(&self, index: usize) {
        if let Some(option) = self
            .prompt
            .options
            .get(index)
            .filter(|option| option.is_shown())
        {
            option.state.set(!option.state.get());
        }
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
                .or(self.completions.selected)
                .unwrap_or(0)
        } else {
            self.selection.selected()
        };
        self.completed_input = Some(input);
        self.in_list &= !self.candidates().is_empty();
        self.select(index);
    }

    /// The selected candidate, else the input, else the default.
    pub(crate) fn answer(&self, literal_input: bool) -> Result<String, CommandError> {
        let answer = self.unchecked_answer(literal_input)?;
        if let Some(check) = &self.prompt.check {
            check(&answer)?;
        }
        Ok(answer)
    }

    fn unchecked_answer(&self, literal_input: bool) -> Result<String, CommandError> {
        if self.prompt.message.is_some() {
            return Ok(self.message_text());
        }
        let input = self.input_text();
        let literal_input = literal_input || self.prompt.list_keymap.is_some();
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

fn input_buffer(name: &str, text: &str) -> Buffer {
    let mut buffer = Buffer::new(name, Text::from(text), FUNDAMENTAL_MODE);
    buffer.set_point(text.len());
    buffer
}

fn replace_text(buffer: &mut Buffer, text: &str) {
    buffer.replace_text(Text::from(text));
    buffer.set_point(text.len());
}

/// A message's first line, and the lines after the blank ones under it.
fn split_message(message: &str) -> (&str, &str) {
    let (subject, description) = message.split_once('\n').unwrap_or((message, ""));
    (subject, description.trim_start_matches('\n'))
}
