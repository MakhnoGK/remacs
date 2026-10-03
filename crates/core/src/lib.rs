//! The editor core: buffers, keymaps, commands, modes and the plugin registry.

mod buffer;
mod command;
mod editor;
mod key;
mod keymap;
mod mode;
mod plugin;
mod viewport;

pub use buffer::Buffer;
pub use command::{Command, CommandError, CommandResult, Commands};
pub use editor::{Editor, SELF_INSERT_COMMAND};
pub use key::{Key, KeyCode, Modifiers, describe_sequence, parse_sequence};
pub use keymap::{Binding, Keymap};
pub use mode::{FUNDAMENTAL_MODE, Mode, Modes};
pub use plugin::{Plugin, Registry};
pub use viewport::Viewport;
