//! The editor core: buffers, keymaps, commands, modes, the minibuffer, the kill ring and
//! the plugin registry.

mod buffer;
mod command;
mod completion;
mod decoration;
mod editor;
mod key;
mod keymap;
mod kill_ring;
mod minibuffer;
mod mode;
mod plugin;
mod viewport;

pub use buffer::Buffer;
pub use command::{Command, CommandError, CommandResult, Commands};
pub use completion::{Candidate, Completions, Icon, Match, filter, filter_candidates, rank};
pub use decoration::{Decorator, Face, Span};
pub use editor::{Editor, Notifier, SCRATCH_BUFFER, SELF_INSERT_COMMAND};
pub use key::{Key, KeyCode, Modifiers, describe_sequence, parse_sequence};
pub use keymap::{Binding, Keymap};
pub use kill_ring::KillRing;
pub use minibuffer::{Count, Minibuffer, Prompt, VISIBLE_CANDIDATES};
pub use mode::{FUNDAMENTAL_MODE, Mode, Modes};
pub use plugin::{Hook, Plugin, Registry};
pub use viewport::Viewport;
