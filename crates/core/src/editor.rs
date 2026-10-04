use std::any::{Any, TypeId};
use std::cell::Cell;
use std::collections::HashMap;
use std::io;
use std::mem;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, PoisonError, RwLock};

use remacs_text::Text;

use crate::buffer::{Buffer, FileEdits};
use crate::command::{CommandResult, Commands};
use crate::completion::Completions;
use crate::decoration::Span;
use crate::key::{Key, describe_sequence};
use crate::keymap::{Binding, Keymap};
use crate::kill_ring::KillRing;
use crate::minibuffer::{Dialog, Minibuffer, Prompt};
use crate::mode::Mode;
use crate::outline::Outline;
use crate::plugin::{Plugin, Registry};
use crate::popup::Popup;
use crate::status::StatusSegment;
use crate::syntax::SyntaxContext;
use crate::table::{Action, Table};
use crate::vc::{FileStatus, VersionControl};
use crate::viewport::Viewport;

pub const SELF_INSERT_COMMAND: &str = "self-insert-command";
pub const SCRATCH_BUFFER: &str = "*scratch*";
const ABORT_KEY: &str = "C-g";
/// Like Emacs, consecutive self-inserts undo together, at most this many at a time.
const MAX_AMALGAMATED_INSERTS: usize = 20;
const MAX_HISTORY: usize = 100;

/// Called from any thread when background work has results; the UI then calls
/// [`Editor::refresh`] on its own thread.
pub type Notifier = Arc<dyn Fn() + Send + Sync>;

pub struct Editor {
    /// Most recently selected first; the first one is shown in the window.
    buffers: Vec<Buffer>,
    minibuffer: Option<Minibuffer>,
    pub viewport: Viewport,
    pub kill_ring: KillRing,
    registry: Registry,
    pending_keys: Vec<Key>,
    last_key: Option<Key>,
    /// The command being run; M-x replaces it with the command it runs, as in Emacs.
    this_command: Option<&'static str>,
    last_command: Option<&'static str>,
    this_command_killed: bool,
    last_command_killed: bool,
    command_count: u64,
    amalgamated_inserts: usize,
    /// Answers given to the minibuffer, newest first, by completion category.
    history: HashMap<&'static str, Vec<String>>,
    message: Option<String>,
    quit_requested: bool,
    plugin_states: HashMap<TypeId, Box<dyn Any>>,
    notifier: Arc<RwLock<Notifier>>,
}

impl Editor {
    pub fn new(plugins: &[&dyn Plugin]) -> Self {
        let mut registry = Registry::default();
        for plugin in plugins {
            plugin.register(&mut registry);
        }
        let scratch = Buffer::new(
            SCRATCH_BUFFER,
            Text::default(),
            registry.modes.name_for(None),
        );
        Self {
            buffers: vec![scratch],
            minibuffer: None,
            viewport: Viewport::default(),
            kill_ring: KillRing::default(),
            registry,
            pending_keys: Vec::new(),
            last_key: None,
            this_command: None,
            last_command: None,
            this_command_killed: false,
            last_command_killed: false,
            command_count: 0,
            amalgamated_inserts: 0,
            history: HashMap::new(),
            message: None,
            quit_requested: false,
            plugin_states: HashMap::new(),
            notifier: Arc::new(RwLock::new(Arc::new(|| {}))),
        }
    }

    /// The buffer commands act on: the minibuffer input, or a message's description,
    /// while a prompt is open.
    pub fn buffer(&self) -> &Buffer {
        match &self.minibuffer {
            Some(minibuffer) => minibuffer.editing(),
            None => &self.buffers[0],
        }
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        match &mut self.minibuffer {
            Some(minibuffer) => minibuffer.editing_mut(),
            None => &mut self.buffers[0],
        }
    }

    pub fn window_buffer(&self) -> &Buffer {
        &self.buffers[0]
    }

    pub fn window_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[0]
    }

    /// All buffers, most recently selected first.
    pub fn buffers(&self) -> &[Buffer] {
        &self.buffers
    }

    pub fn buffers_mut(&mut self) -> &mut [Buffer] {
        &mut self.buffers
    }

    pub fn buffer_named_mut(&mut self, name: &str) -> Option<&mut Buffer> {
        self.buffers.iter_mut().find(|buffer| buffer.name() == name)
    }

    pub fn mode_for(&self, path: Option<&Path>) -> &'static str {
        self.registry.modes.name_for(path)
    }

    /// The current buffer's major mode; none for Fundamental.
    pub fn buffer_mode(&self) -> Option<&Mode> {
        self.registry.modes.get(self.buffer().mode())
    }

    /// Whether `offset` in the current buffer is in a string, a comment or code; code
    /// when no plugin can tell.
    pub fn syntax_at(&self, offset: usize) -> SyntaxContext {
        let buffer = self.buffer();
        self.registry
            .syntax_probes
            .iter()
            .find_map(|probe| probe(buffer, offset))
            .unwrap_or(SyntaxContext::Code)
    }

    /// Shows the buffer visiting `path`, opening it first if needed; a missing file
    /// becomes an empty buffer that creates it on save.
    pub fn visit_file(&mut self, path: &Path) -> io::Result<()> {
        let index = self.find_file(path)?;
        self.select_buffer(index);
        Ok(())
    }

    /// The buffer visiting `path`, opened without showing it if needed, like Emacs
    /// `find-file-noselect`.
    pub fn find_file_noselect(&mut self, path: &Path) -> io::Result<&mut Buffer> {
        let index = self.find_file(path)?;
        Ok(&mut self.buffers[index])
    }

    /// Makes `edits` in the buffer visiting their file, opened without showing it if
    /// needed; they undo together and are left unsaved.
    pub fn edit_file(&mut self, edits: FileEdits) -> io::Result<&mut Buffer> {
        let buffer = self.find_file_noselect(&edits.path)?;
        buffer.add_undo_boundary();
        buffer.apply_edits(edits.edits);
        Ok(buffer)
    }

    /// Makes the window buffer visit `path` from now on, renamed after it, like Emacs
    /// `set-visited-file-name`; saving is up to the caller.
    pub fn set_visited_file(&mut self, path: &Path) -> io::Result<()> {
        let path = path.canonicalize().or_else(|_| std::path::absolute(path))?;
        if let Some(other) = self.buffers[1..]
            .iter()
            .find(|buffer| buffer.path() == Some(&path))
        {
            let message = format!(
                "Buffer {} is already visiting {}",
                other.name(),
                path.display()
            );
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, message));
        }
        let name = file_buffer_name(&path);
        if name != self.buffers[0].name() {
            let name = self.unique_name(&name);
            self.buffers[0].set_name(name);
        }
        let mode = self.mode_for(Some(&path));
        self.buffers[0].set_file(path, mode);
        Ok(())
    }

    /// Makes buffers visiting `from`, or files under it, visit them at `to`, renamed
    /// after them, as Emacs does after `dired-rename-file`.
    pub fn file_moved(&mut self, from: &Path, to: &Path) {
        for index in 0..self.buffers.len() {
            let Some(rest) = self.buffers[index]
                .path()
                .and_then(|path| path.strip_prefix(from).ok())
            else {
                continue;
            };
            let path = if rest.as_os_str().is_empty() {
                to.to_path_buf()
            } else {
                to.join(rest)
            };
            let mode = self.mode_for(Some(&path));
            let name = file_buffer_name(&path);
            if name != self.buffers[index].name() {
                let name = self.unique_name(&name);
                self.buffers[index].set_name(name);
            }
            self.buffers[index].set_file(path, mode);
        }
    }

    /// Adds `buffer` under a unique name (`name<2>`) and shows it.
    pub fn add_buffer(&mut self, buffer: Buffer) {
        let index = self.push_buffer(buffer);
        self.select_buffer(index);
    }

    pub fn switch_to_buffer(&mut self, name: &str) -> bool {
        self.buffer_index(name)
            .inspect(|&index| self.select_buffer(index))
            .is_some()
    }

    /// Removes a buffer; the window falls back to the next most recent one, or `*scratch*`.
    pub fn kill_buffer(&mut self, name: &str) -> bool {
        let Some(index) = self.buffer_index(name) else {
            return false;
        };
        self.buffers.remove(index);
        if self.buffers.is_empty() {
            let mode = self.mode_for(None);
            self.buffers
                .push(Buffer::new(SCRATCH_BUFFER, Text::default(), mode));
        }
        if index == 0 {
            self.viewport.first_line = self.buffers[0].window_start;
        }
        true
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

    pub fn handle_key(&mut self, key: Key) {
        self.message = None;
        if Key::parse(ABORT_KEY) == Some(key) && self.abort() {
            return;
        }
        if self
            .minibuffer
            .as_ref()
            .is_some_and(Minibuffer::is_question)
        {
            self.answer_question_key(key);
            return;
        }
        if self.pending_keys.is_empty()
            && let Some(index) = self
                .minibuffer
                .as_ref()
                .and_then(|minibuffer| minibuffer.option_for_key(key))
        {
            self.toggle_prompt_option(index);
            return;
        }
        if self.pending_keys.is_empty()
            && let Some(index) = self
                .minibuffer
                .as_ref()
                .and_then(|minibuffer| minibuffer.choice_for_key(key))
        {
            self.next_prompt_choice(index);
            return;
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
                let typed = self.minibuffer.as_ref().is_none_or(Minibuffer::is_typed);
                if typed && keys.len() == 1 && key.printable_char().is_some() {
                    self.run_command(SELF_INSERT_COMMAND);
                } else {
                    self.set_message(format!("{} is undefined", describe_sequence(&keys)));
                }
            }
        }
    }

    pub fn run_command(&mut self, name: &str) {
        let Some(command) = self.registry.commands.get(name) else {
            self.set_message(format!("Unknown command: {name}"));
            return;
        };
        let (name, run) = (command.name, command.run);
        self.start_undo_group(name);
        self.this_command = Some(name);
        self.this_command_killed = false;
        if let Err(error) = run(self) {
            self.set_message(error.0);
        }
        self.last_command = self.this_command;
        self.last_command_killed = self.this_command_killed;
        self.command_count += 1;
        self.after_command();
    }

    pub fn commands(&self) -> &Commands {
        &self.registry.commands
    }

    pub fn keymap(&self) -> &Keymap {
        &self.registry.keymap
    }

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
            self.set_message(error.0);
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
    /// the candidates may depend on it, so they are listed again.
    pub fn toggle_prompt_option(&mut self, index: usize) {
        if let Some(minibuffer) = &self.minibuffer {
            minibuffer.toggle_prompt_option(index);
            self.refresh_minibuffer();
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
        self.minibuffer.take().is_some()
    }

    /// Answers the prompt with the candidate at `index`, as a mouse click does.
    pub fn choose_candidate(&mut self, index: usize) {
        if let Some(minibuffer) = &mut self.minibuffer {
            minibuffer.select(index);
        }
        if let Err(error) = self.exit_minibuffer(false) {
            self.set_message(error.0);
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
        self.history.get(category).map_or(&[], Vec::as_slice)
    }

    /// Scrolls without moving point unless it would leave the window.
    pub fn scroll(&mut self, lines: isize) {
        let line_count = self.window_buffer().text().line_count();
        if self.viewport.scroll_by(lines, line_count) {
            let point_line = self.window_buffer().point_line();
            let visible_line = self.viewport.clamp(point_line);
            if visible_line != point_line {
                self.window_buffer_mut().move_to_line(visible_line);
            }
        }
    }

    pub fn set_point_at(&mut self, line: usize, column: usize) {
        let buffer = self.window_buffer_mut();
        let offset = buffer.text().offset_at_char_column(line, column);
        buffer.set_point(offset);
        buffer.deactivate_mark();
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

    /// The echo area: a command's message, or a prefix typed so far (`C-x-`).
    pub fn echo(&self) -> Option<String> {
        if self.pending_keys.is_empty() {
            self.message.clone()
        } else {
            Some(format!("{}-", describe_sequence(&self.pending_keys)))
        }
    }

    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    pub fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// State a plugin keeps between commands, created on first use.
    pub fn plugin_state<T: Default + 'static>(&mut self) -> &mut T {
        self.plugin_states
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(T::default()))
            .downcast_mut()
            .expect("plugin states are keyed by their type")
    }

    /// A plugin's state, if it has created any.
    pub fn find_plugin_state<T: 'static>(&self) -> Option<&T> {
        self.plugin_states.get(&TypeId::of::<T>())?.downcast_ref()
    }

    pub fn set_notifier(&mut self, notifier: Notifier) {
        *self
            .notifier
            .write()
            .unwrap_or_else(PoisonError::into_inner) = notifier;
    }

    /// Calls whichever notifier is set when it is called, so work started before the UI
    /// exists, e.g. for a directory given on the command line, still wakes it.
    pub fn notifier(&self) -> Notifier {
        let notifier = Arc::clone(&self.notifier);
        Arc::new(move || (notifier.read().unwrap_or_else(PoisonError::into_inner))())
    }

    /// Picks up what background work has produced since the last call.
    pub fn refresh(&mut self) {
        self.refresh_minibuffer();
        self.run_post_command_hooks();
        // A regenerated buffer, like a directory listing, may have moved point.
        let point_line = self.window_buffer().point_line();
        self.viewport.ensure_visible(point_line);
    }

    /// Runs a table control's action as if it were a command.
    pub fn run_action(&mut self, action: &Action) {
        self.message = None;
        if let Err(error) = action(self) {
            self.set_message(error.0);
        }
        self.after_command();
    }

    /// `lines` of the window buffer as a table, if a plugin draws it as one.
    pub fn table(&self, lines: Range<usize>) -> Option<Table> {
        self.registry
            .tables
            .iter()
            .find_map(|describe| describe(self, lines.clone()))
    }

    /// `lines` of the window buffer as an outline, if a plugin draws it as one.
    pub fn outline(&self, lines: Range<usize>) -> Option<Outline> {
        self.registry
            .outlines
            .iter()
            .find_map(|describe| describe(self, lines.clone()))
    }

    /// The version control keeping `path`, if any.
    pub fn version_control(&self, path: &Path) -> Option<Rc<dyn VersionControl>> {
        self.registry
            .version_controls
            .iter()
            .find(|control| control.controls(self, path))
            .cloned()
    }

    /// How `path` differs from the last commit, if version control knows.
    pub fn file_status(&self, path: &Path) -> Option<FileStatus> {
        self.version_control(path)?.status(self, path)
    }

    /// Tells version control of files moved, from and to, as its move command would.
    pub fn record_moves(&mut self, moves: &[(PathBuf, PathBuf)]) {
        for control in self.registry.version_controls.clone() {
            let kept: Vec<_> = moves
                .iter()
                .filter(|(from, _)| {
                    self.version_control(from)
                        .is_some_and(|keeper| Rc::ptr_eq(&keeper, &control))
                })
                .cloned()
                .collect();
            if !kept.is_empty() {
                control.record_moves(self, &kept);
            }
        }
    }

    /// Faces for `range` of the window buffer, one sorted list per decorator.
    pub fn decorations(&self, range: Range<usize>) -> Vec<Vec<Span>> {
        self.registry
            .decorators
            .iter()
            .map(|decorate| decorate(self.window_buffer(), range.clone()))
            .collect()
    }

    pub fn status_segments(&self) -> Vec<StatusSegment> {
        self.registry
            .status_providers
            .iter()
            .filter_map(|describe| describe(self))
            .collect()
    }

    pub fn popups(&self) -> Vec<Popup> {
        self.registry
            .popup_providers
            .iter()
            .filter_map(|describe| describe(self))
            .collect()
    }

    /// Faces for `code` written in `mode`, sorted and non-overlapping; none when no
    /// highlighter knows the mode.
    pub fn highlight_code(&self, mode: &'static str, code: &str) -> Vec<Span> {
        self.registry
            .code_highlighters
            .iter()
            .find_map(|highlight| highlight(mode, code))
            .unwrap_or_default()
    }

    fn run_post_command_hooks(&mut self) {
        for hook in self.registry.post_command_hooks.clone() {
            hook(self);
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

    /// C-g cancels a pending prefix or the prompt; false when there was neither.
    fn abort(&mut self) -> bool {
        if !self.pending_keys.is_empty() {
            self.pending_keys.clear();
        } else if !self.cancel_minibuffer() {
            return false;
        }
        self.set_message("Quit");
        true
    }

    fn answer_question_key(&mut self, key: Key) {
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
        self.refresh_minibuffer();
        Ok(())
    }

    fn finish_minibuffer(&mut self, answer: String) -> CommandResult {
        let minibuffer = self.minibuffer.take().ok_or("Not in the minibuffer")?;
        let category = minibuffer.prompt().category;
        if !category.is_empty() {
            let history = self.history.entry(category).or_default();
            history.retain(|earlier| *earlier != answer);
            history.insert(0, answer.clone());
            history.truncate(MAX_HISTORY);
        }
        (minibuffer.on_answer)(self, answer)
    }

    fn after_command(&mut self) {
        if self
            .minibuffer
            .as_ref()
            .is_some_and(Minibuffer::needs_completions)
        {
            self.refresh_minibuffer();
        }
        let point_line = self.window_buffer().point_line();
        self.viewport.ensure_visible(point_line);
        self.run_post_command_hooks();
    }

    /// Index of the buffer visiting `path`, opened as the least recent one if needed.
    fn find_file(&mut self, path: &Path) -> io::Result<usize> {
        let path = path.canonicalize().or_else(|_| std::path::absolute(path))?;
        if let Some(index) = self
            .buffers
            .iter()
            .position(|buffer| buffer.path() == Some(&path))
        {
            return Ok(index);
        }
        let text = match Text::open(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.set_message("(New file)");
                Text::default()
            }
            Err(error) => return Err(error),
        };
        let mode = self.mode_for(Some(&path));
        Ok(self.push_buffer(Buffer::new(file_buffer_name(&path), text, mode).with_file(path)))
    }

    /// Adds `buffer` as the least recent one under a unique name and returns its index.
    fn push_buffer(&mut self, mut buffer: Buffer) -> usize {
        buffer.set_name(self.unique_name(buffer.name()));
        self.buffers.push(buffer);
        self.buffers.len() - 1
    }

    fn buffer_index(&self, name: &str) -> Option<usize> {
        self.buffers.iter().position(|buffer| buffer.name() == name)
    }

    fn select_buffer(&mut self, index: usize) {
        self.buffers[0].window_start = self.viewport.first_line;
        let buffer = self.buffers.remove(index);
        self.viewport.first_line = buffer.window_start;
        self.buffers.insert(0, buffer);
    }

    fn unique_name(&self, name: &str) -> String {
        let taken = |candidate: &str| self.buffer_index(candidate).is_some();
        if !taken(name) {
            return name.to_owned();
        }
        (2..)
            .map(|number| format!("{name}<{number}>"))
            .find(|candidate| !taken(candidate))
            .expect("unbounded")
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

fn file_buffer_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn buffers_are_most_recent_first_with_unique_names() {
        let mut editor = Editor::new(&[]);
        editor.add_buffer(Buffer::new("a", Text::default(), "Fundamental"));
        editor.add_buffer(Buffer::new("a", Text::default(), "Fundamental"));
        let names = |editor: &Editor| -> Vec<String> {
            editor
                .buffers()
                .iter()
                .map(|buffer| buffer.name().to_owned())
                .collect()
        };
        assert_eq!(names(&editor), ["a<2>", "a", SCRATCH_BUFFER]);
        assert!(editor.switch_to_buffer(SCRATCH_BUFFER));
        assert_eq!(names(&editor), [SCRATCH_BUFFER, "a<2>", "a"]);
        assert!(editor.kill_buffer(SCRATCH_BUFFER));
        assert!(editor.kill_buffer("a<2>") && editor.kill_buffer("a"));
        assert_eq!(names(&editor), [SCRATCH_BUFFER]);
    }

    #[test]
    fn buffers_follow_their_files_when_moved() {
        let mut editor = Editor::new(&[]);
        let file = |path: &str| {
            Buffer::new("x", Text::default(), "Fundamental").with_file(PathBuf::from(path))
        };
        editor.add_buffer(file("/a/one.rs"));
        editor.add_buffer(file("/a/b/two.rs"));
        editor.add_buffer(file("/ab/three.rs"));
        editor.file_moved(Path::new("/a"), Path::new("/c"));
        editor.file_moved(Path::new("/c/one.rs"), Path::new("/c/renamed.rs"));
        let visited: Vec<_> = editor
            .buffers()
            .iter()
            .filter_map(|buffer| Some((buffer.name(), buffer.path()?.to_str()?)))
            .collect();
        assert_eq!(
            visited,
            [
                ("x<3>", "/ab/three.rs"),
                ("two.rs", "/c/b/two.rs"),
                ("renamed.rs", "/c/renamed.rs")
            ]
        );
    }

    #[test]
    fn a_notifier_taken_early_calls_the_one_set_later() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let mut editor = Editor::new(&[]);
        let early = editor.notifier();
        let called = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&called);
        editor.set_notifier(Arc::new(move || flag.store(true, Ordering::Relaxed)));
        early();
        assert!(called.load(Ordering::Relaxed));
    }

    #[test]
    fn buffers_remember_their_scroll_position() {
        let mut editor = Editor::new(&[]);
        editor.viewport.first_line = 7;
        editor.add_buffer(Buffer::new("other", Text::default(), "Fundamental"));
        assert_eq!(editor.viewport.first_line, 0);
        editor.switch_to_buffer(SCRATCH_BUFFER);
        assert_eq!(editor.viewport.first_line, 7);
    }
}
