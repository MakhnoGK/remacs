use std::any::{Any, TypeId};
use std::cell::Cell;
use std::collections::HashMap;
use std::io;
use std::mem;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, Instant, SystemTime};

use remacs_config::{Section, Settings, project_cache_dir};
use remacs_text::Text;

use crate::buffer::{Buffer, FileEdits};
use crate::command::{CommandResult, Commands};
use crate::completion::Completions;
use crate::decoration::Span;
use crate::echo::{EchoMessage, MessageKind, MessageLog};
use crate::gutter::{GutterMark, LineExtra, TextControls};
use crate::history::History;
use crate::key::{Key, describe_sequence};
use crate::key_bindings::KeysSettings;
use crate::keymap::{Binding, Keymap};
use crate::kill_ring::KillRing;
use crate::menu::{self, FrameRequest, MenuLayout};
use crate::minibuffer::{Dialog, Minibuffer, Prompt};
use crate::mode::Mode;
use crate::outline::Outline;
use crate::pane::Pane;
use crate::plugin::{Plugin, Registry};
use crate::popup::Popup;
use crate::screen::{Screen, ScreenMouse, ScreenPadding, ScreenPointer, ScreenSize};
use crate::status::StatusSegment;
use crate::syntax::SyntaxContext;
use crate::table::{Action, Table};
use crate::vc::{ChangedLines, FileStatus, VersionControl};
use crate::viewport::Viewport;

pub const SELF_INSERT_COMMAND: &str = "self-insert-command";
pub const SCRATCH_BUFFER: &str = "*scratch*";
const ABORT_KEY: &str = "C-g";
/// Like Emacs, consecutive self-inserts undo together, at most this many at a time.
const MAX_AMALGAMATED_INSERTS: usize = 20;

/// Called from any thread when background work has results; the UI then calls
/// [`Editor::refresh`] on its own thread.
pub type Notifier = Arc<dyn Fn() + Send + Sync>;

pub struct Editor {
    /// Most recently selected first; the first one is shown in the window.
    buffers: Vec<Buffer>,
    minibuffer: Option<Minibuffer>,
    /// The prompt last closed, until the view takes it to draw it going away.
    closed_minibuffer: Option<Minibuffer>,
    pub viewport: Viewport,
    pub kill_ring: KillRing,
    registry: Registry,
    /// The plugins' bindings, under those of `keys.toml`.
    default_keymap: Keymap,
    pending_keys: Vec<Key>,
    last_key: Option<Key>,
    /// The command being run; M-x replaces it with the command it runs, as in Emacs.
    this_command: Option<&'static str>,
    last_command: Option<&'static str>,
    this_command_killed: bool,
    last_command_killed: bool,
    command_count: u64,
    amalgamated_inserts: usize,
    history: History,
    /// The echo area's message, until the next key.
    message: Option<EchoMessage>,
    messages: MessageLog,
    /// The view lists all its keys, not only those that fit, until the next key.
    key_hints_listed: bool,
    quit_requested: bool,
    frame_requests: Vec<FrameRequest>,
    plugin_states: HashMap<TypeId, Box<dyn Any>>,
    settings: Settings,
    cache_dir: Option<PathBuf>,
    notifier: Arc<RwLock<Notifier>>,
    refresh_at: Option<Instant>,
    /// Files moved since [`Editor::take_moved_files`], from and to.
    moved_files: Vec<(PathBuf, PathBuf)>,
    /// The pane last chosen in the switch between panes.
    selected_pane: Option<&'static str>,
    /// The shown pane has the keys, not the window buffer.
    pane_focused: bool,
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
            closed_minibuffer: None,
            viewport: Viewport::default(),
            kill_ring: KillRing::default(),
            default_keymap: registry.keymap.clone(),
            registry,
            pending_keys: Vec::new(),
            last_key: None,
            this_command: None,
            last_command: None,
            this_command_killed: false,
            last_command_killed: false,
            command_count: 0,
            amalgamated_inserts: 0,
            history: History::default(),
            message: None,
            messages: MessageLog::default(),
            key_hints_listed: false,
            quit_requested: false,
            frame_requests: Vec::new(),
            plugin_states: HashMap::new(),
            settings: Settings::default(),
            cache_dir: None,
            notifier: Arc::new(RwLock::new(Arc::new(|| {}))),
            refresh_at: None,
            moved_files: Vec::new(),
            selected_pane: None,
            pane_focused: false,
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

    /// Files moved since the last call, from and to, for backends such as language
    /// servers to be told.
    pub fn take_moved_files(&mut self) -> Vec<(PathBuf, PathBuf)> {
        mem::take(&mut self.moved_files)
    }

    /// Makes buffers visiting `from`, or files under it, visit them at `to`, renamed
    /// after them, as Emacs does after `dired-rename-file`.
    pub fn file_moved(&mut self, from: &Path, to: &Path) {
        self.moved_files
            .push((from.to_path_buf(), to.to_path_buf()));
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

    /// Adds `buffer` under a unique name without showing it, as the least recent one;
    /// returns the name.
    pub fn add_buffer_noselect(&mut self, buffer: Buffer) -> String {
        let index = self.push_buffer(buffer);
        self.buffers[index].name().to_owned()
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

    /// False when the key is a sequence of its own that nothing binds, nor types, so
    /// that it may still mean something to the system, e.g. a menu's key equivalent.
    pub fn handle_key(&mut self, key: Key) -> bool {
        self.clear_echo();
        if Key::parse(ABORT_KEY) == Some(key) && self.abort() {
            // Hooks see a prompt closed by quitting, e.g. to undo what it previewed.
            self.after_command();
            return true;
        }
        if self
            .minibuffer
            .as_ref()
            .is_some_and(Minibuffer::is_question)
        {
            self.answer_question_key(key);
            return true;
        }
        if self.pending_keys.is_empty()
            && let Some(index) = self
                .minibuffer
                .as_ref()
                .and_then(|minibuffer| minibuffer.option_for_key(key))
        {
            self.toggle_prompt_option(index);
            return true;
        }
        if self.pending_keys.is_empty()
            && let Some(index) = self
                .minibuffer
                .as_ref()
                .and_then(|minibuffer| minibuffer.choice_for_key(key))
        {
            self.next_prompt_choice(index);
            return true;
        }
        if self.pending_keys.is_empty() && self.minibuffer.is_none() {
            for sink in self.registry.key_sinks.clone() {
                if sink(self, key) {
                    self.last_key = Some(key);
                    return true;
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
                let typed = match &self.minibuffer {
                    Some(minibuffer) => minibuffer.is_typed(),
                    None => !self
                        .registry
                        .typing_blockers
                        .iter()
                        .any(|blocks| blocks(self)),
                };
                if typed && keys.len() == 1 && key.printable_char().is_some() {
                    self.run_command(SELF_INSERT_COMMAND);
                } else {
                    self.set_message(format!("{} is undefined", describe_sequence(&keys)));
                    return keys.len() > 1;
                }
            }
        }
        true
    }

    pub fn run_command(&mut self, name: &str) {
        let Some(command) = self.registry.commands.get(name) else {
            self.set_error(format!("Unknown command: {name}"));
            return;
        };
        let (name, run) = (command.name, command.run);
        self.start_undo_group(name);
        self.this_command = Some(name);
        self.this_command_killed = false;
        if let Err(error) = run(self) {
            self.set_error(error.0);
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
        let Some(minibuffer) = self.minibuffer.take() else {
            return false;
        };
        self.closed_minibuffer = Some(minibuffer);
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

    /// Lets plugins keep what they can rebuild under `dir`, e.g. a project's file index.
    pub fn keep_caches_in(&mut self, dir: PathBuf) {
        self.cache_dir = Some(dir);
    }

    /// The directory a plugin may keep the project at `root`'s caches in; none until
    /// [`Editor::keep_caches_in`], e.g. in tests.
    pub fn project_cache_dir(&self, root: &Path) -> Option<PathBuf> {
        let cache = self.cache_dir.as_deref()?;
        Some(project_cache_dir(cache, root))
    }

    /// Scrolls without moving point unless it would leave the window.
    pub fn scroll(&mut self, lines: isize) {
        let line_count = self.window_buffer().text().line_count();
        if self.viewport.scroll_by(lines, line_count) {
            let point_line = self.window_buffer().point_line();
            let visible_line = self.viewport.clamp(point_line, line_count);
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

    /// The echo area as text: the message, or the prefix typed so far (`C-x-`).
    pub fn echo(&self) -> Option<String> {
        if self.pending_keys.is_empty() {
            self.message.as_ref().map(|message| message.text.clone())
        } else {
            Some(format!("{}-", describe_sequence(&self.pending_keys)))
        }
    }

    /// The echo area's message, shown until the next key.
    pub fn message(&self) -> Option<&EchoMessage> {
        self.message.as_ref()
    }

    /// Takes the message out of the echo area before the next key; `*Messages*` keeps it.
    pub fn clear_message(&mut self) {
        self.message = None;
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

    pub fn set_message(&mut self, text: impl Into<String>) {
        self.show_message(text.into(), MessageKind::Info);
    }

    /// A message saying something asked for was done, e.g. a file written.
    pub fn set_done(&mut self, text: impl Into<String>) {
        self.show_message(text.into(), MessageKind::Done);
    }

    pub fn set_error(&mut self, text: impl Into<String>) {
        self.show_message(text.into(), MessageKind::Error);
    }

    /// Every message shown, for `*Messages*`.
    pub fn message_log(&self) -> &MessageLog {
        &self.messages
    }

    pub fn message_log_mut(&mut self) -> &mut MessageLog {
        &mut self.messages
    }

    /// Lists all the view's keys, not only those that fit under it, until the next key.
    pub fn list_key_hints(&mut self) {
        self.key_hints_listed = true;
    }

    pub fn key_hints_listed(&self) -> bool {
        self.key_hints_listed
    }

    fn show_message(&mut self, text: String, kind: MessageKind) {
        let message = EchoMessage {
            text,
            kind,
            command: self.this_command,
            at: SystemTime::now(),
            repeats: 0,
        };
        self.messages.add(&message);
        self.message = Some(message);
    }

    /// What the echo area showed for the last key is gone with the next one.
    fn clear_echo(&mut self) {
        self.message = None;
        self.key_hints_listed = false;
    }

    pub fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub fn request_frame(&mut self, request: FrameRequest) {
        self.frame_requests.push(request);
    }

    pub fn take_frame_requests(&mut self) -> Vec<FrameRequest> {
        mem::take(&mut self.frame_requests)
    }

    pub fn menu_bar(&self) -> Vec<MenuLayout<'_>> {
        menu::layout(&self.registry.menus, &self.registry.menu_items)
    }

    /// Whether the menu items running `command` can be chosen now.
    pub fn menu_command_enabled(&self, command: &str) -> bool {
        self.registry.commands.get(command).is_some()
            && self
                .registry
                .menu_items
                .iter()
                .filter(|item| item.command == command)
                .all(|item| item.enabled.is_none_or(|enabled| enabled(self)))
    }

    /// Replaces the settings and lets plugins read their sections again, then refreshes,
    /// so that they act on them without waiting for a command, e.g. restart a server.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
        let keys = self.settings_section::<KeysSettings>();
        let (keymap, errors) = keys.apply(&self.default_keymap, &self.registry.commands);
        self.registry.keymap = keymap;
        if let Some(error) = errors.into_iter().next() {
            self.set_error(error);
        }
        for hook in self.registry.settings_hooks.clone() {
            hook(self);
        }
        self.refresh();
    }

    /// A plugin's section of the settings; when it is wrong, its defaults, and the error
    /// is shown.
    pub fn settings_section<T: Section>(&mut self) -> T {
        self.settings
            .section::<T>()
            .unwrap_or_else(|(error, defaults)| {
                self.set_error(error.to_string());
                defaults
            })
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

    /// Asks the UI to call [`Editor::refresh`] once `delay` has passed, like an Emacs idle
    /// timer; hooks then tell a quiet moment by what changed meanwhile.
    pub fn refresh_after(&mut self, delay: Duration) {
        let at = Instant::now() + delay;
        self.refresh_at = Some(self.refresh_at.map_or(at, |earlier| earlier.min(at)));
    }

    /// The earliest refresh asked for since the last call.
    pub fn take_refresh_at(&mut self) -> Option<Instant> {
        self.refresh_at.take()
    }

    /// Picks up what background work has produced since the last call.
    pub fn refresh(&mut self) {
        self.refresh_minibuffer();
        self.run_post_command_hooks();
        // A regenerated buffer, like a directory listing, may have moved point.
        self.show_point();
    }

    /// Runs a table control's action as if it were a command.
    pub fn run_action(&mut self, action: &Action) {
        self.clear_echo();
        if let Err(error) = action(self) {
            self.set_error(error.0);
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

    /// Marks beside `lines` of the window buffer, sorted by line; none when no plugin has
    /// marks for the buffer.
    pub fn gutter_marks(&self, lines: Range<usize>) -> Option<Vec<GutterMark>> {
        let mut marks: Option<Vec<GutterMark>> = None;
        for provide in &self.registry.gutters {
            if let Some(more) = provide(self, lines.clone()) {
                marks.get_or_insert_default().extend(more);
            }
        }
        if let Some(marks) = &mut marks {
            marks.sort_by_key(|mark| mark.line);
        }
        marks
    }

    pub fn line_extras(&self, lines: Range<usize>) -> Vec<LineExtra> {
        self.registry
            .line_extras
            .iter()
            .flat_map(|provide| provide(self, lines.clone()))
            .collect()
    }

    /// The pane shown under the window buffer: the one last selected while it has
    /// something to show, else the first that has.
    pub fn shown_pane(&self) -> Option<&'static str> {
        let mut shown = self
            .registry
            .panes
            .iter()
            .filter(|source| (source.shown)(self))
            .map(|source| source.name);
        let first = shown.next();
        match self.selected_pane {
            Some(selected) if first == Some(selected) || shown.any(|name| name == selected) => {
                Some(selected)
            }
            _ => first,
        }
    }

    /// The shown pane, described.
    pub fn pane(&self) -> Option<Pane> {
        let name = self.shown_pane()?;
        let source = self
            .registry
            .panes
            .iter()
            .find(|source| source.name == name)?;
        (source.describe)(self)
    }

    /// The panes with something to show, in order, and their counts, for the switch
    /// between them; none while only one has.
    pub fn pane_switch(&self) -> Vec<(&'static str, Option<usize>)> {
        let shown: Vec<_> = self
            .registry
            .panes
            .iter()
            .filter(|source| (source.shown)(self))
            .collect();
        if shown.len() < 2 {
            return Vec::new();
        }
        shown
            .into_iter()
            .map(|source| {
                let count = (source.describe)(self).and_then(|pane| pane.count);
                (source.name, count)
            })
            .collect()
    }

    /// Shows the pane named `name` whenever it has something to show.
    pub fn select_pane(&mut self, name: &'static str) {
        self.selected_pane = Some(name);
    }

    /// Gives the keys to the shown pane, or back to the window buffer.
    pub fn focus_pane(&mut self, focused: bool) {
        self.pane_focused = focused;
    }

    /// Whether the shown pane has the keys, unless a prompt has them.
    pub fn pane_focused(&self) -> bool {
        self.pane_focused && self.minibuffer.is_none() && self.shown_pane().is_some()
    }

    /// Whether the keys go to `buffer()`, the prompt's or the window's, not to a pane;
    /// a keymap of the window buffer's mode holds only while they do.
    pub fn buffer_has_keys(&self) -> bool {
        !self.pane_focused()
    }

    /// Whether the pane named `name` is shown and has the keys.
    pub fn pane_has_keys(&self, name: &str) -> bool {
        self.pane_focused() && self.shown_pane() == Some(name)
    }

    /// The buffer named `name` as a screen of `size`, if a plugin draws it as one.
    pub fn screen(&self, name: &str, size: ScreenSize) -> Option<Screen> {
        let buffer = self.buffers.iter().find(|buffer| buffer.name() == name)?;
        self.registry
            .screens
            .iter()
            .find_map(|source| source.screen(self, buffer, size))
    }

    /// Whether the buffer named `name` is drawn as a screen.
    pub fn is_screen(&self, name: &str) -> bool {
        self.buffers
            .iter()
            .find(|buffer| buffer.name() == name)
            .is_some_and(|buffer| {
                self.registry
                    .screens
                    .iter()
                    .any(|source| source.draws(buffer))
            })
    }

    /// Room around the cells of the buffer named `name`, drawn as a screen.
    pub fn screen_padding(&self, name: &str) -> ScreenPadding {
        let Some(buffer) = self.buffers.iter().find(|buffer| buffer.name() == name) else {
            return ScreenPadding::default();
        };
        self.registry
            .screens
            .iter()
            .find(|source| source.draws(buffer))
            .map(|source| source.padding(self))
            .unwrap_or_default()
    }

    /// Passes the mouse over a screen's cell to the plugins that draw screens; true when
    /// the screen changed with it. A move changes nothing else.
    pub fn screen_mouse(&mut self, name: &str, pointer: ScreenPointer) -> bool {
        let moved = matches!(pointer.mouse, ScreenMouse::Move | ScreenMouse::Leave);
        if matches!(pointer.mouse, ScreenMouse::Press { .. }) {
            self.clear_echo();
        }
        let mut changed = false;
        for source in self.registry.screens.clone() {
            changed |= source.mouse(self, name, pointer);
        }
        if !moved {
            self.after_command();
        }
        changed
    }

    /// Renames the buffer named `name`, as `name<2>` if the new name is taken.
    pub fn rename_buffer(&mut self, name: &str, new_name: &str) -> Option<String> {
        let index = self.buffer_index(name)?;
        if name == new_name {
            return Some(new_name.to_owned());
        }
        let unique = self.unique_name(new_name);
        self.buffers[index].set_name(unique.clone());
        Some(unique)
    }

    /// What a click beside a line without a gutter mark runs, if anything.
    pub fn gutter_click(&self) -> Option<&'static str> {
        self.registry.gutter_click
    }

    /// Controls of the window buffer, if a plugin has any for it.
    pub fn text_controls(&self) -> Option<TextControls> {
        self.registry
            .text_controls
            .iter()
            .find_map(|describe| describe(self))
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

    /// How the window buffer's `lines` differ from the last commit, if version control
    /// knows.
    pub fn line_changes(&self, lines: Range<usize>) -> Vec<ChangedLines> {
        let buffer = self.window_buffer();
        buffer
            .path()
            .and_then(|path| self.version_control(path))
            .map(|control| control.line_changes(buffer, lines))
            .unwrap_or_default()
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
        self.popup_slots().flatten().collect()
    }

    /// Each plugin's popup, if it has one open, in the same place every time, so a popup
    /// is known as the same one from one frame to the next.
    pub fn popup_slots(&self) -> impl Iterator<Item = Option<Popup>> + '_ {
        self.registry
            .popup_providers
            .iter()
            .map(|describe| describe(self))
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
        on_answer(self, answer)
    }

    fn after_command(&mut self) {
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

    /// Scrolls the window buffer's point into view, out of the margins.
    pub fn show_point(&mut self) {
        let buffer = self.window_buffer();
        let (point_line, line_count) = (buffer.point_line(), buffer.text().line_count());
        self.viewport.ensure_visible(point_line, line_count);
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
        // Left as it was, a short buffer would stay scrolled sideways.
        self.viewport.first_column = 0;
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
    fn buffers_remember_their_scroll_position() {
        let mut editor = Editor::new(&[]);
        editor.viewport.first_line = 7;
        editor.add_buffer(Buffer::new("other", Text::default(), "Fundamental"));
        assert_eq!(editor.viewport.first_line, 0);
        editor.switch_to_buffer(SCRATCH_BUFFER);
        assert_eq!(editor.viewport.first_line, 7);
    }
}
