use std::io;
use std::mem;
use std::path::Path;

use remacs_text::Text;

use crate::buffer::Buffer;
use crate::key::{Key, describe_sequence};
use crate::keymap::Binding;
use crate::plugin::{Plugin, Registry};
use crate::viewport::Viewport;

pub const SELF_INSERT_COMMAND: &str = "self-insert-command";
const ABORT_KEY: &str = "C-g";
/// Like Emacs, consecutive self-inserts undo together, at most this many at a time.
const MAX_AMALGAMATED_INSERTS: usize = 20;

pub struct Editor {
    pub buffer: Buffer,
    pub viewport: Viewport,
    registry: Registry,
    pending_keys: Vec<Key>,
    last_key: Option<Key>,
    last_command: Option<&'static str>,
    amalgamated_inserts: usize,
    message: Option<String>,
    quit_requested: bool,
}

impl Editor {
    pub fn new(plugins: &[&dyn Plugin]) -> Self {
        let mut registry = Registry::default();
        for plugin in plugins {
            plugin.register(&mut registry);
        }
        let mode = registry.modes.name_for(None);
        Self {
            buffer: Buffer::new("*scratch*", Text::default(), mode),
            viewport: Viewport::default(),
            registry,
            pending_keys: Vec::new(),
            last_key: None,
            last_command: None,
            amalgamated_inserts: 0,
            message: None,
            quit_requested: false,
        }
    }

    pub fn visit_file(&mut self, path: &Path) -> io::Result<()> {
        let text = Text::open(path)?;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        self.buffer = Buffer::new(name, text, self.registry.modes.name_for(Some(path)));
        self.viewport.first_line = 0;
        Ok(())
    }

    pub fn handle_key(&mut self, key: Key) {
        self.message = None;
        if Key::parse(ABORT_KEY) == Some(key) && !self.pending_keys.is_empty() {
            self.pending_keys.clear();
            self.set_message("Quit");
            return;
        }
        self.pending_keys.push(key);
        self.last_key = Some(key);
        match self.registry.keymap.lookup(&self.pending_keys) {
            Binding::Command(name) => {
                self.pending_keys.clear();
                self.run_command(name);
            }
            Binding::Prefix => {}
            Binding::Unbound => {
                let keys = mem::take(&mut self.pending_keys);
                if keys.len() == 1 && key.printable_char().is_some() {
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
        if let Err(error) = run(self) {
            self.set_message(error.0);
        }
        self.last_command = Some(name);
        self.viewport.ensure_visible(self.buffer.point_line());
    }

    /// Scrolls without moving point unless it would leave the window.
    pub fn scroll(&mut self, lines: isize) {
        let line_count = self.buffer.text().line_count();
        if self.viewport.scroll_by(lines, line_count) {
            let point_line = self.buffer.point_line();
            let visible_line = self.viewport.clamp(point_line);
            if visible_line != point_line {
                self.buffer.move_to_line(visible_line);
            }
        }
    }

    pub fn set_point_at(&mut self, line: usize, column: usize) {
        let offset = self.buffer.text().offset_at_char_column(line, column);
        self.buffer.set_point(offset);
    }

    pub fn last_key(&self) -> Option<Key> {
        self.last_key
    }

    pub fn last_command(&self) -> Option<&'static str> {
        self.last_command
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

    fn start_undo_group(&mut self, command: &'static str) {
        let amalgamate = command == SELF_INSERT_COMMAND
            && self.last_command == Some(SELF_INSERT_COMMAND)
            && self.amalgamated_inserts < MAX_AMALGAMATED_INSERTS;
        if amalgamate {
            self.amalgamated_inserts += 1;
        } else {
            self.amalgamated_inserts = 0;
            self.buffer.add_undo_boundary();
        }
    }
}
