use std::collections::HashMap;

use crate::editor::Editor;

pub type CommandResult = Result<(), CommandError>;

/// A user-facing failure, shown in the echo area.
#[derive(Debug)]
pub struct CommandError(pub String);

/// From a message, or an error as it describes itself, e.g. an `io::Error`.
impl<T: ToString> From<T> for CommandError {
    fn from(message: T) -> Self {
        Self(message.to_string())
    }
}

#[derive(Clone, Copy)]
pub struct Command {
    pub name: &'static str,
    pub doc: &'static str,
    pub run: fn(&mut Editor) -> CommandResult,
    /// Runs again at each of the buffer's cursors, like a command multiple-cursors does not
    /// list as run once.
    pub each_cursor: bool,
}

impl Command {
    pub const fn new(
        name: &'static str,
        doc: &'static str,
        run: fn(&mut Editor) -> CommandResult,
    ) -> Self {
        Self { name, doc, run, each_cursor: false }
    }

    pub const fn each_cursor(mut self) -> Self {
        self.each_cursor = true;
        self
    }
}

#[derive(Default)]
pub struct Commands(HashMap<&'static str, Command>);

impl Commands {
    pub fn add(&mut self, command: Command) {
        self.0.insert(command.name, command);
    }

    pub fn get(&self, name: &str) -> Option<&Command> {
        self.0.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Command> {
        self.0.values()
    }
}
