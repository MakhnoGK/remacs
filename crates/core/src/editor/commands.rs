use std::collections::HashMap;
use std::mem;
use std::ops::Range;

use super::{Editor, SELF_INSERT_COMMAND};
use crate::command::{CommandResult, Commands};
use crate::key::{Key, KeyCode, Modifiers, describe_sequence};
use crate::keymap::{Binding, Keymap};
use crate::minibuffer::Minibuffer;
use crate::plugin::NextErrorSource;
use crate::table::Action;

const ABORT_KEY: &str = "C-g";
/// Like Emacs, consecutive self-inserts undo together, at most this many at a time.
const MAX_AMALGAMATED_INSERTS: usize = 20;

/// What `Editor::dispatch_key` did with a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOutcome {
    Handled,
    /// A key of its own that nothing binds, nor types, so that it may still mean something
    /// to the system, e.g. a menu's key equivalent.
    Unhandled,
    /// Nothing binds it and it types its character: the caller's to insert.
    Typed,
}

impl Editor {
    /// Text another program put on the clipboard becomes the newest kill, at every cursor
    /// too, as each yanks from a kill ring of its own; what remacs copied there itself
    /// does not, so the cursors keep their own kills.
    pub fn receive_clipboard(&mut self, text: String) {
        if !self.kill_ring.receive(text.clone()) {
            return;
        }
        let buffer = &mut self.buffers[0];
        for index in 0..buffer.cursors().len() {
            buffer.cursor_kill_ring(index).receive(text.clone());
        }
    }

    /// Deletes `range` from the current buffer into the kill ring; consecutive kills
    /// accumulate in one entry, before it when killing backward from point.
    pub fn kill(&mut self, range: Range<usize>) {
        let buffer = self.buffer_mut();
        let backward = buffer.point() == range.end && range.start < range.end;
        let text = buffer.contents(range.clone());
        buffer.delete(range);
        if self.last_command_killed {
            self.kill_ring.append(&text, backward);
        } else {
            self.kill_ring.push(text);
        }
        self.this_command_killed = true;
    }

    /// False when the key is a sequence of its own that nothing binds, nor types, so
    /// that it may still mean something to the system, e.g. a menu's key equivalent.
    pub fn handle_key(&mut self, key: Key) -> bool {
        match self.dispatch_key(key) {
            KeyOutcome::Typed => {
                self.run_command(SELF_INSERT_COMMAND);
                true
            }
            outcome => outcome == KeyOutcome::Handled,
        }
    }

    /// Like `handle_key`, but a key that would type its character is left to the caller,
    /// which has the input method turn it into text for `type_text`.
    pub fn dispatch_key(&mut self, key: Key) -> KeyOutcome {
        self.clear_echo();
        if Key::parse(ABORT_KEY) == Some(key) && self.abort() {
            // Hooks see a prompt closed by quitting, e.g. to undo what it previewed.
            self.after_command();
            return KeyOutcome::Handled;
        }
        if self.key_reader.is_some() {
            self.read_key(key);
            return KeyOutcome::Handled;
        }
        if self
            .minibuffer
            .as_ref()
            .is_some_and(Minibuffer::is_question)
        {
            self.answer_question_key(key);
            return KeyOutcome::Handled;
        }
        if self.pending_keys.is_empty()
            && let Some(index) = self
                .minibuffer
                .as_ref()
                .and_then(|minibuffer| minibuffer.option_for_key(key))
        {
            self.toggle_prompt_option(index);
            return KeyOutcome::Handled;
        }
        if self.pending_keys.is_empty()
            && let Some(index) = self
                .minibuffer
                .as_ref()
                .and_then(|minibuffer| minibuffer.choice_for_key(key))
        {
            self.next_prompt_choice(index);
            return KeyOutcome::Handled;
        }
        if self.pending_keys.is_empty() && self.minibuffer.is_none() {
            for sink in self.registry.key_sinks.clone() {
                if sink(self, key) {
                    self.last_key = Some(key);
                    return KeyOutcome::Handled;
                }
            }
        }
        self.pending_keys.push(key);
        self.last_key = Some(key);
        match Keymap::lookup_in(&self.active_keymaps(), &self.pending_keys) {
            Binding::Command(name) => {
                self.pending_keys.clear();
                self.run_command(name);
            }
            Binding::Prefix => {}
            Binding::Unbound => {
                let keys = mem::take(&mut self.pending_keys);
                if self.takes_typing() && keys.len() == 1 && key.printable_char().is_some() {
                    // Refused at once: the input method would compose text, and swallow the
                    // keys after it, for a buffer that cannot take it.
                    if self.buffer().is_read_only() {
                        self.run_command(SELF_INSERT_COMMAND);
                        return KeyOutcome::Handled;
                    }
                    return KeyOutcome::Typed;
                }
                self.set_message(format!("{} is undefined", describe_sequence(&keys)));
                if keys.len() == 1 {
                    return KeyOutcome::Unhandled;
                }
            }
        }
        KeyOutcome::Handled
    }

    /// Has `then` take the next key sequence, read until it runs a command or nothing,
    /// instead of running it, like Emacs `read-key-sequence`; C-g cancels.
    pub fn read_key_sequence(
        &mut self,
        then: impl FnOnce(&mut Editor, &[Key], Option<&'static str>) + 'static,
    ) {
        self.key_reader = Some(Box::new(then));
    }

    fn read_key(&mut self, key: Key) {
        self.pending_keys.push(key);
        let command = match Keymap::lookup_in(&self.active_keymaps(), &self.pending_keys) {
            Binding::Prefix => return,
            Binding::Command(name) => Some(name),
            Binding::Unbound => (self.pending_keys.len() == 1
                && key.printable_char().is_some()
                && self.takes_typing())
            .then_some(SELF_INSERT_COMMAND),
        };
        let keys = mem::take(&mut self.pending_keys);
        if let Some(then) = self.key_reader.take() {
            then(self, &keys, command);
        }
        self.after_command();
    }

    /// Inserts text an input method made, e.g. a composed or accented character, or one
    /// from the emoji picker, each character as if its key had been typed; a newline, as
    /// dictation gives one, as `RET`.
    pub fn type_text(&mut self, text: &str) {
        let in_prompt = self.minibuffer.is_some();
        for character in text.chars() {
            // Refused once, as a key is; a newline would still run what RET does there, e.g.
            // open a file the rest would land in.
            if let Err(error) = self.buffer().ensure_writable() {
                self.set_error(error.0);
                return;
            }
            // A newline may have submitted a prompt, opened one, or opened something typing
            // is not for: the rest was not meant for it.
            if !self.pending_keys.is_empty()
                || !self.takes_typing()
                || self.minibuffer.is_some() != in_prompt
            {
                return;
            }
            if character == '\n' {
                self.handle_key(Key::new(Modifiers::default(), KeyCode::Return));
            } else if !character.is_control() {
                self.last_key = Some(Key::new(Modifiers::default(), KeyCode::Char(character)));
                self.run_command(SELF_INSERT_COMMAND);
            }
        }
    }

    /// Whether a key nothing binds types its character.
    fn takes_typing(&self) -> bool {
        match &self.minibuffer {
            Some(minibuffer) => minibuffer.is_typed(),
            None => !self
                .registry
                .typing_blockers
                .iter()
                .any(|blocks| blocks(self)),
        }
    }

    pub fn run_command(&mut self, name: &str) {
        let Some(command) = self.registry.commands.get(name) else {
            self.set_error(format!("Unknown command: {name}"));
            return;
        };
        let (name, run, each_cursor) = (command.name, command.run, command.each_cursor);
        // A key reader left by a command run since, e.g. from the menu, is not waited on.
        self.key_reader = None;
        self.start_undo_group(name);
        self.this_command = Some(name);
        self.this_command_killed = false;
        if each_cursor {
            // A command run once, like undo or a kill, may have moved cursors together.
            self.buffers[0].merge_cursors();
        }
        if let Err(error) = run(self) {
            self.set_error(error.0);
        } else if each_cursor && self.minibuffer.is_none() {
            self.run_at_cursors(run);
        }
        self.last_command = self.this_command;
        self.last_command_killed = self.this_command_killed;
        self.command_count += 1;
        self.after_command();
    }

    /// Runs a command again at each cursor of the buffer, each with its own point, mark and
    /// kill ring; what fails at one is not told, as only point's result is.
    fn run_at_cursors(&mut self, run: fn(&mut Editor) -> CommandResult) {
        let message = self.message.take();
        for index in 0..self.buffers[0].cursors().len() {
            self.buffers[0].swap_cursor(index);
            mem::swap(&mut self.kill_ring, self.buffers[0].cursor_kill_ring(index));
            let _ = run(self);
            mem::swap(&mut self.kill_ring, self.buffers[0].cursor_kill_ring(index));
            self.buffers[0].swap_cursor(index);
        }
        self.buffers[0].merge_cursors();
        self.message = message;
    }

    pub fn commands(&self) -> &Commands {
        &self.registry.commands
    }

    pub fn keymap(&self) -> &Keymap {
        &self.registry.keymap
    }

    /// Where `next-error` goes in place of the buffer's own problems, if anywhere.
    pub fn next_error_source(&self) -> Option<NextErrorSource> {
        let name = self.next_error_source?;
        let sources = &self.registry.next_error_sources;
        sources
            .iter()
            .find(|source| source.name == name && (source.active)(self))
            .copied()
    }

    /// Makes `next-error` go to the source named, as when it made or visited locations, or
    /// with `None` to the buffer's own problems.
    pub fn set_next_error_source(&mut self, name: Option<&'static str>) {
        self.next_error_source = name;
    }

    pub fn last_key(&self) -> Option<Key> {
        self.last_key
    }

    pub fn last_command(&self) -> Option<&'static str> {
        self.last_command
    }

    /// Commands run so far; post-command hooks compare it to tell a command from a
    /// background refresh.
    pub fn command_count(&self) -> u64 {
        self.command_count
    }

    /// The keys of a prefix typed so far, e.g. `C-x`; empty when none is.
    pub fn pending_keys(&self) -> &[Key] {
        &self.pending_keys
    }

    /// What each key typed after the pending prefix would do, highest keymap first, like
    /// the list Emacs `which-key` shows.
    pub fn prefix_continuations(&self) -> Vec<(Key, Binding)> {
        let mut seen = HashMap::new();
        for keymap in self.active_keymaps() {
            for (key, binding) in keymap.continuations(&self.pending_keys) {
                seen.entry(key).or_insert(binding);
            }
        }
        seen.into_iter().collect()
    }

    /// Runs a table control's action as if it were a command.
    pub fn run_action(&mut self, action: &Action) {
        self.clear_echo();
        if let Err(error) = action(self) {
            self.set_error(error.0);
        }
        self.after_command();
    }

    pub(super) fn run_post_command_hooks(&mut self) {
        for hook in self.registry.post_command_hooks.clone() {
            hook(self);
        }
        self.run_display_hooks();
        self.scrolled_to = None;
        // A pane shown later must not find the keys already given to it.
        if self.shown_pane().is_none() {
            self.pane_focused = false;
        }
    }

    fn active_keymaps(&self) -> Vec<&Keymap> {
        let message_keymap = self
            .minibuffer
            .as_ref()
            .filter(|minibuffer| minibuffer.description().is_some())
            .map(|_| &self.registry.message_keymap);
        let minibuffer_keymap = self
            .minibuffer
            .as_ref()
            .map(|_| &self.registry.minibuffer_keymap);
        let minor_keymaps = self
            .registry
            .minor_keymaps
            .iter()
            .filter(|minor| (minor.active)(self))
            .map(|minor| &minor.keymap);
        let list_keymap = self.minibuffer.as_ref().and_then(Minibuffer::keymap);
        let prompt_keymap = self
            .minibuffer
            .as_ref()
            .and_then(|minibuffer| minibuffer.prompt().keymap.as_ref());
        list_keymap
            .into_iter()
            .chain(prompt_keymap)
            .chain(message_keymap)
            .chain(minibuffer_keymap)
            .chain(minor_keymaps)
            .chain([&self.registry.keymap])
            .collect()
    }

    /// C-g cancels reading a key, a pending prefix or the prompt; false when there was none.
    fn abort(&mut self) -> bool {
        let reading_key = self.key_reader.take().is_some();
        if reading_key || !self.pending_keys.is_empty() {
            self.pending_keys.clear();
        } else if !self.cancel_minibuffer() {
            return false;
        }
        self.set_message("Quit");
        true
    }

    pub(super) fn after_command(&mut self) {
        if self
            .minibuffer
            .as_ref()
            .is_some_and(Minibuffer::needs_completions)
        {
            self.refresh_minibuffer();
        }
        self.show_point();
        self.run_post_command_hooks();
    }

    fn start_undo_group(&mut self, command: &'static str) {
        let amalgamate = command == SELF_INSERT_COMMAND
            && self.last_command == Some(SELF_INSERT_COMMAND)
            && self.amalgamated_inserts < MAX_AMALGAMATED_INSERTS;
        if amalgamate {
            self.amalgamated_inserts += 1;
        } else {
            self.amalgamated_inserts = 0;
            self.buffer_mut().add_undo_boundary();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{Plugin, Registry};

    #[test]
    fn lists_what_the_keys_after_a_prefix_do() {
        struct Bindings;
        impl Plugin for Bindings {
            fn register(&self, registry: &mut Registry) {
                registry
                    .commands
                    .add(crate::Command::new("one", "", |_| Ok(())));
                for keys in ["C-x a", "C-x r b", "C-y"] {
                    registry.keymap.bind(keys, "one");
                }
            }
        }
        let mut editor = Editor::new(&[&Bindings]);
        editor.handle_key(Key::parse("C-x").unwrap());
        let mut listed: Vec<_> = editor
            .prefix_continuations()
            .into_iter()
            .map(|(key, binding)| match binding {
                Binding::Command(command) => format!("{key} {command}"),
                Binding::Prefix => format!("{key} …"),
                Binding::Unbound => format!("{key} unbound"),
            })
            .collect();
        listed.sort();
        assert_eq!(listed, ["a one", "r …"]);
        assert_eq!(editor.echo().as_deref(), Some("C-x-"));
    }

    #[test]
    fn reads_a_key_sequence_whole_instead_of_running_it() {
        struct Bindings;
        impl Plugin for Bindings {
            fn register(&self, registry: &mut Registry) {
                registry
                    .commands
                    .add(crate::Command::new("one", "", |_| Err("ran".into())));
                registry.keymap.bind("C-x a", "one");
            }
        }
        let mut editor = Editor::new(&[&Bindings]);
        let read = |editor: &mut Editor| {
            editor.read_key_sequence(|editor, keys, command| {
                editor.set_message(format!("{} {command:?}", describe_sequence(keys)));
            });
        };
        let press = |editor: &mut Editor, notation| {
            assert_eq!(
                editor.dispatch_key(Key::parse(notation).unwrap()),
                KeyOutcome::Handled
            );
        };
        read(&mut editor);
        press(&mut editor, "C-x");
        press(&mut editor, "a");
        assert_eq!(editor.echo().as_deref(), Some("C-x a Some(\"one\")"));
        read(&mut editor);
        press(&mut editor, "C-y");
        assert_eq!(editor.echo().as_deref(), Some("C-y None"));
        read(&mut editor);
        press(&mut editor, "C-x");
        press(&mut editor, "C-g");
        press(&mut editor, "C-x");
        press(&mut editor, "a");
        assert_eq!(editor.echo().as_deref(), Some("ran"));
        // A command run otherwise, e.g. from the menu, drops the reader.
        read(&mut editor);
        editor.run_command("one");
        press(&mut editor, "C-x");
        press(&mut editor, "a");
        assert_eq!(editor.echo().as_deref(), Some("ran"));
    }

    #[test]
    fn only_a_single_key_nothing_binds_is_left_to_the_system() {
        struct Bindings;
        impl Plugin for Bindings {
            fn register(&self, registry: &mut Registry) {
                registry
                    .commands
                    .add(crate::Command::new("one", "", |_| Ok(())));
                registry.keymap.bind("C-x a", "one");
                registry.keymap.bind("s-s", "one");
            }
        }
        let mut editor = Editor::new(&[&Bindings]);
        let mut press = |notation| editor.handle_key(Key::parse(notation).unwrap());
        assert!(press("s-s"));
        assert!(!press("s-m"));
        assert!(press("C-x"));
        assert!(press("s-m"));
    }

    #[test]
    fn a_typing_key_is_left_to_the_input_method_whose_text_self_inserts() {
        struct Typing;
        impl Plugin for Typing {
            fn register(&self, registry: &mut Registry) {
                registry
                    .commands
                    .add(crate::Command::new(SELF_INSERT_COMMAND, "", |editor| {
                        let character = editor
                            .last_key()
                            .and_then(|key| key.printable_char())
                            .ok_or("Not printable")?;
                        editor.buffer().ensure_writable()?;
                        editor
                            .buffer_mut()
                            .insert(character.encode_utf8(&mut [0; 4]));
                        Ok(())
                    }));
                registry
                    .commands
                    .add(crate::Command::new("newline", "", |editor| {
                        if editor.minibuffer().is_some() {
                            return editor.exit_minibuffer(true);
                        }
                        editor.buffer_mut().insert("\n");
                        Ok(())
                    }));
                registry.keymap.bind("RET", "newline");
            }
        }
        let mut editor = Editor::new(&[&Typing]);
        let mut dispatch = |notation| editor.dispatch_key(Key::parse(notation).unwrap());
        assert_eq!(dispatch("a"), KeyOutcome::Typed);
        assert_eq!(dispatch("M-a"), KeyOutcome::Unhandled);
        editor.type_text("é\u{1}愛\nb");
        let buffer = editor.buffer();
        assert_eq!(buffer.contents(0..buffer.text().len()), "é愛\nb");
        // What follows a newline that answers a prompt was not meant for the buffer.
        let none = |_: &Editor, _: &str| crate::Completions::default();
        let prompt = crate::minibuffer::Prompt::default();
        editor.read(prompt, none, |_, _| Ok(())).unwrap();
        editor.type_text("x\ny");
        assert!(editor.minibuffer().is_none());
        let buffer = editor.buffer();
        assert_eq!(buffer.contents(0..buffer.text().len()), "é愛\nb");
        editor.buffer_mut().set_read_only(true);
        assert_eq!(
            editor.dispatch_key(Key::parse("a").unwrap()),
            KeyOutcome::Handled
        );
        editor.type_text("a\nb");
        let buffer = editor.buffer();
        assert_eq!(buffer.contents(0..buffer.text().len()), "é愛\nb");
    }
}
