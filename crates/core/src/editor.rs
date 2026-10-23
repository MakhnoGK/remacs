use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::mem;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, Instant};

use remacs_config::Settings;
use remacs_text::Text;

use crate::buffer::Buffer;
use crate::echo::{EchoMessage, MessageLog};
use crate::history::History;
use crate::key::Key;
use crate::keymap::Keymap;
use crate::kill_ring::KillRing;
use crate::menu::{self, FrameRequest, MenuLayout};
use crate::minibuffer::Minibuffer;
use crate::plugin::{Plugin, Registry};
use crate::viewport::Viewport;
use crate::window::Windows;

mod buffers;
mod commands;
mod display;
mod messages;
mod modes;
mod prompts;
mod settings;
mod windows;

pub use commands::KeyOutcome;
use windows::Displayed;

pub const SELF_INSERT_COMMAND: &str = "self-insert-command";
pub const SCRATCH_BUFFER: &str = "*scratch*";

/// Called from any thread when background work has results; the UI then calls
/// [`Editor::refresh`] on its own thread.
pub type Notifier = Arc<dyn Fn() + Send + Sync>;

/// Given the key sequence read whole and the command it runs, if any; see
/// [`Editor::read_key_sequence`].
pub type KeyReader = Box<dyn FnOnce(&mut Editor, &[Key], Option<&'static str>)>;

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
    /// Takes the next key sequence instead of running it.
    key_reader: Option<KeyReader>,
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
    /// See [`Editor::scrolled_to`].
    scrolled_to: Option<(String, usize)>,
    windows: Windows,
    /// See [`Editor::display_window`].
    displayed: Option<Displayed>,
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
            key_reader: None,
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
            scrolled_to: None,
            windows: Windows::default(),
            displayed: None,
        }
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
            // Another window's point is not where the selected one read on to.
            if self.displayed.is_some() {
                return;
            }
            let buffer = &self.buffers[0];
            match &mut self.scrolled_to {
                Some((name, offset)) if name == buffer.name() => *offset = buffer.point(),
                scrolled_to => *scrolled_to = Some((buffer.name().to_owned(), buffer.point())),
            }
        }
    }

    /// The buffer the window's last scroll, by a command or the mouse, was in since the
    /// post-command hooks last ran, and where it left point: point moved there by reading
    /// on, not by going somewhere.
    pub fn scrolled_to(&self) -> Option<(&str, usize)> {
        let (name, offset) = self.scrolled_to.as_ref()?;
        Some((name, *offset))
    }

    pub fn set_point_at(&mut self, line: usize, column: usize) {
        let buffer = self.window_buffer_mut();
        let offset = buffer.text().offset_at_char_column(line, column);
        buffer.set_point(offset);
        buffer.deactivate_mark();
    }

    /// Scrolls the window buffer's point into view, out of the margins.
    pub fn show_point(&mut self) {
        let buffer = self.window_buffer();
        let (point_line, line_count) = (buffer.point_line(), buffer.text().line_count());
        self.viewport.ensure_visible(point_line, line_count);
    }

    pub fn request_quit(&mut self) {
        self.quit_requested = true;
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// The buffers whose programs quitting now would end, all plugins' together.
    pub fn running_programs(&self) -> Vec<String> {
        self.registry
            .quit_queries
            .iter()
            .flat_map(|query| query(self))
            .collect()
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
