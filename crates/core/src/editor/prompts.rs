use std::cell::Cell;
use std::mem;
use std::path::PathBuf;
use std::rc::Rc;

use super::Editor;
use crate::command::CommandResult;
use crate::completion::Completions;
use crate::key::Key;
use crate::minibuffer::{Dialog, Minibuffer, Prompt};

impl Editor {
    /// Opens a prompt; `complete` lists candidates for the input as it changes and
    /// `on_answer` receives the chosen value.
    pub fn read(
        &mut self,
        prompt: Prompt,
        complete: impl Fn(&Editor, &str) -> Completions + 'static,
        on_answer: impl FnOnce(&mut Editor, String) -> CommandResult + 'static,
    ) -> CommandResult {
        self.open_minibuffer(Minibuffer::new(
            prompt,
            Some(Rc::new(complete)),
            Box::new(on_answer),
        ))
    }

    /// Asks a question answered with a single `y` or `n`, like Emacs `y-or-n-p`.
    pub fn ask_y_or_n(
        &mut self,
        question: impl Into<String>,
        on_answer: impl FnOnce(&mut Editor, bool) -> CommandResult + 'static,
    ) -> CommandResult {
        self.open_minibuffer(Minibuffer::question(
            question.into(),
            Box::new(move |editor, answer| on_answer(editor, answer == "y")),
        ))
    }

    /// Asks a y-or-n question drawn as a dialog; `describe` draws it with its option on
    /// or off, and `on_answer` gets the answer and the option as it was then.
    pub fn ask_dialog(
        &mut self,
        option_on: bool,
        describe: impl Fn(bool) -> Dialog + 'static,
        on_answer: impl FnOnce(&mut Editor, bool, bool) -> CommandResult + 'static,
    ) -> CommandResult {
        let option = Rc::new(Cell::new(option_on));
        let answered_option = Rc::clone(&option);
        self.open_minibuffer(Minibuffer::dialog_question(
            Rc::new(describe),
            option,
            Box::new(move |editor, answer| on_answer(editor, answer == "y", answered_option.get())),
        ))
    }

    /// Answers the open question, as a click on its buttons does.
    pub fn answer_question(&mut self, yes: bool) {
        if !self
            .minibuffer
            .as_ref()
            .is_some_and(Minibuffer::is_question)
        {
            return;
        }
        let answer = if yes { "y" } else { "n" };
        if let Err(error) = self.finish_minibuffer(answer.to_owned()) {
            self.set_error(error.0);
        }
        self.after_command();
    }

    /// Switches the open dialog's option, as a click on it does.
    pub fn toggle_dialog_option(&mut self) {
        if let Some(minibuffer) = &self.minibuffer {
            minibuffer.toggle_option(None);
        }
    }

    /// Switches option `index` of the open prompt, as `M-` and its key or a click does;
    /// the candidates may depend on it, so they are listed again, and so may what the
    /// post-command hooks follow, e.g. a search's preview, so they run.
    pub fn toggle_prompt_option(&mut self, index: usize) {
        if let Some(minibuffer) = &self.minibuffer {
            minibuffer.toggle_prompt_option(index);
            self.refresh_minibuffer();
            self.run_post_command_hooks();
        }
    }

    /// Picks the next value of choice `index` of the open prompt, as its key or a click
    /// does; the candidates may depend on it, so they are listed again.
    pub fn next_prompt_choice(&mut self, index: usize) {
        if let Some(minibuffer) = &self.minibuffer {
            minibuffer.next_choice(index);
            self.refresh_minibuffer();
        }
    }

    pub fn minibuffer(&self) -> Option<&Minibuffer> {
        self.minibuffer.as_ref()
    }

    pub fn minibuffer_mut(&mut self) -> Option<&mut Minibuffer> {
        self.minibuffer.as_mut()
    }

    /// Answers the prompt with the selected candidate, or with the input as typed.
    pub fn exit_minibuffer(&mut self, literal_input: bool) -> CommandResult {
        let minibuffer = self.minibuffer.as_ref().ok_or("Not in the minibuffer")?;
        let answer = minibuffer.answer(literal_input)?;
        self.finish_minibuffer(answer)
    }

    /// Closes the prompt without answering it.
    pub fn cancel_minibuffer(&mut self) -> bool {
        let Some(minibuffer) = self.minibuffer.take() else {
            return false;
        };
        self.closed_minibuffer = Some(minibuffer);
        self.hide_shown_in_prompt();
        true
    }

    /// The prompt closed since the last call, if none is open now, for the view to draw as
    /// it goes away; it no longer takes input.
    pub fn take_closed_minibuffer(&mut self) -> Option<Minibuffer> {
        self.closed_minibuffer.take()
    }

    /// Answers the prompt with the candidate at `index`, as a mouse click does.
    pub fn choose_candidate(&mut self, index: usize) {
        if let Some(minibuffer) = &mut self.minibuffer {
            minibuffer.select(index);
        }
        if let Err(error) = self.exit_minibuffer(false) {
            self.set_error(error.0);
        }
        self.after_command();
    }

    /// Selects the candidate at `index` and moves the keys to the list, as a click on an
    /// entry does.
    pub fn focus_candidate(&mut self, index: usize) {
        if let Some(minibuffer) = &mut self.minibuffer {
            minibuffer.focus_candidate(index);
        }
    }

    /// Lists candidates again, e.g. after a command changed what they describe.
    pub fn refresh_minibuffer(&mut self) {
        let Some(minibuffer) = &self.minibuffer else {
            return;
        };
        let Some(complete) = minibuffer.source.clone() else {
            return;
        };
        let input = minibuffer.input_text();
        let completions = complete(self, &input);
        if let Some(minibuffer) = &mut self.minibuffer {
            minibuffer.set_completions(completions, input);
        }
    }

    /// Earlier answers in a completion category, newest first.
    pub fn history(&self, category: &str) -> &[String] {
        self.history.get(category)
    }

    /// Keeps the minibuffer history in `path` across sessions: reads it now and writes it
    /// after each answer.
    pub fn keep_history_in(&mut self, path: PathBuf) {
        if let Err(error) = self.history.keep_in(path) {
            self.set_error(error);
        }
    }

    pub(super) fn answer_question_key(&mut self, key: Key) {
        match key.printable_char() {
            Some('y') => self.answer_question(true),
            Some('n') => self.answer_question(false),
            key if self
                .minibuffer
                .as_ref()
                .is_some_and(|minibuffer| key.is_some() && minibuffer.toggle_option(key)) => {}
            _ => self.set_message("Please answer y or n"),
        }
    }

    fn open_minibuffer(&mut self, minibuffer: Minibuffer) -> CommandResult {
        if self.minibuffer.is_some() {
            return Err("Command attempted to use minibuffer while in minibuffer".into());
        }
        self.minibuffer = Some(minibuffer);
        // Another prompt in its place does not go away.
        self.closed_minibuffer = None;
        self.refresh_minibuffer();
        Ok(())
    }

    fn finish_minibuffer(&mut self, answer: String) -> CommandResult {
        let mut minibuffer = self.minibuffer.take().ok_or("Not in the minibuffer")?;
        let prompt = minibuffer.prompt();
        if !prompt.category.is_empty()
            && !prompt.private
            && let Err(error) = self.history.add(prompt.category, answer.clone())
        {
            self.set_error(error);
        }
        let on_answer = mem::replace(&mut minibuffer.on_answer, Box::new(|_, _| Ok(())));
        // Before the answer, which may open another prompt.
        self.closed_minibuffer = Some(minibuffer);
        // What an answered prompt showed stays shown.
        self.shown_in_prompt.clear();
        on_answer(self, answer)
    }
}

#[cfg(test)]
mod tests {
    use remacs_text::Text;

    use super::*;
    use crate::buffer::Buffer;

    #[test]
    fn a_closed_prompt_is_handed_over_unless_another_takes_its_place() {
        let mut editor = Editor::new(&[]);
        let prompt = |label: &str| Prompt {
            label: label.to_owned(),
            ..Prompt::default()
        };
        let label = |minibuffer: Option<Minibuffer>| minibuffer.map(|m| m.prompt().label.clone());
        let none = |_: &Editor, _: &str| Completions::default();
        editor
            .read(prompt("cancelled"), none, |_, _| Ok(()))
            .unwrap();
        assert!(editor.take_closed_minibuffer().is_none(), "still open");
        editor.cancel_minibuffer();
        assert_eq!(
            label(editor.take_closed_minibuffer()).as_deref(),
            Some("cancelled")
        );
        assert!(editor.take_closed_minibuffer().is_none(), "taken once");

        editor
            .read(prompt("answered"), none, |_, _| Ok(()))
            .unwrap();
        editor.exit_minibuffer(true).unwrap();
        assert_eq!(
            label(editor.take_closed_minibuffer()).as_deref(),
            Some("answered")
        );

        editor
            .read(prompt("first"), none, move |editor, _| {
                editor.read(prompt("second"), none, |_, _| Ok(()))
            })
            .unwrap();
        editor.exit_minibuffer(true).unwrap();
        assert!(
            editor.take_closed_minibuffer().is_none(),
            "the second took its place"
        );
        assert_eq!(editor.minibuffer().unwrap().prompt().label, "second");
    }

    /// A search previewing a match in a fold shows it until the search is quit.
    #[test]
    fn a_fold_shown_while_a_prompt_is_open_is_hidden_again_if_it_is_quit() {
        let mut editor = Editor::new(&[]);
        let text = "a {\n  b\n}\n";
        editor.add_buffer(Buffer::new("t", Text::from(text), "Fundamental"));
        let folded = |editor: &Editor| editor.window_buffer().shown_lines().is_hidden(1);
        let preview = |editor: &mut Editor, answer: bool| {
            editor.window_buffer_mut().fold(Some(1..2));
            let none = |_: &Editor, _: &str| Completions::default();
            editor.read(Prompt::default(), none, |_, _| Ok(())).unwrap();
            editor
                .window_buffer_mut()
                .set_point(text.find('b').unwrap());
            editor.show_point();
            assert!(!folded(editor), "shown while the prompt is open");
            editor.window_buffer_mut().set_point(0);
            if answer {
                editor.exit_minibuffer(true).unwrap();
            } else {
                editor.cancel_minibuffer();
            }
        };
        preview(&mut editor, false);
        assert!(folded(&editor), "hidden again once quit");
        editor.window_buffer_mut().unfold_all();
        preview(&mut editor, true);
        assert!(!folded(&editor), "left shown once answered");
    }
}
