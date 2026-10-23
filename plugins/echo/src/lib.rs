//! Built-in plugin: the echo area's companions. `*Messages*` keeps every message shown,
//! like Emacs `view-echo-area-messages`; `C-h b` lists the keys of the view that do not
//! fit under it; `C-h k` and `C-h f` tell what a key or a command does.

mod help;
mod messages;

use std::rc::Rc;

use remacs_core::{
    Command, CommandResult, Editor, Keymap, MenuItem, MinorKeymap, Plugin, Registry,
};

pub use messages::{MESSAGES_BUFFER, VIEW as VIEW_MESSAGES};

pub struct EchoPlugin;

const LIST_KEYS: &str = "list-view-keys";

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(messages::VIEW, "Show every message the echo area has shown", messages::view),
    Command::new(messages::CLEAR, "Forget the messages shown so far", messages::clear),
    Command::new(messages::QUIT, "Show the buffer shown before *Messages*", messages::quit),
    Command::new(LIST_KEYS, "List all the keys of the view, also those that do not fit under it", list_keys),
    Command::new(help::DESCRIBE_KEY, "Read a key sequence and tell the command it runs", help::describe_key),
    Command::new(help::DESCRIBE_COMMAND, "Read a command name and tell what it does and its keys", help::describe_command),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-h e", messages::VIEW),
    ("C-h b", LIST_KEYS),
    ("C-h k", help::DESCRIBE_KEY),
    ("C-h f", help::DESCRIBE_COMMAND),
    ("C-h x", help::DESCRIBE_COMMAND),
];

const MESSAGES_BINDINGS: &[(&str, &str)] = &[("q", messages::QUIT), ("C-c C-k", messages::CLEAR)];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Help", "help", "Describe Key…", help::DESCRIBE_KEY),
    MenuItem::new("Help", "help", "Describe Command…", help::DESCRIBE_COMMAND),
    MenuItem::new("Help", "help", "Messages", messages::VIEW),
    MenuItem::new("Help", "help", "Keys of This View", LIST_KEYS),
];

impl Plugin for EchoPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        let mut keymap = Keymap::default();
        for &(keys, command) in MESSAGES_BINDINGS {
            keymap.bind(keys, command);
        }
        registry.minor_keymaps.push(MinorKeymap {
            active: messages::is_shown,
            keymap,
        });
        registry.post_command_hooks.push(Rc::new(messages::update));
        registry.decorators.push(Rc::new(messages::decorate));
        registry.text_controls.push(Rc::new(messages::controls));
    }
}

fn list_keys(editor: &mut Editor) -> CommandResult {
    editor.list_key_hints();
    Ok(())
}

#[cfg(test)]
mod tests;
