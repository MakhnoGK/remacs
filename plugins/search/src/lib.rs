//! Built-in plugin: searching the buffer, its matching lines listed in a prompt like the
//! other searches, like Emacs `consult-line`, and replacing in it, like `query-replace`.

mod lines;
mod preview;
mod query;
mod replace;

use std::rc::Rc;

use remacs_core::{Command, Plugin, Registry};

pub struct SearchPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(lines::SEARCH, "Search the buffer, listing the lines that match", lines::search),
    Command::new(replace::QUERY_REPLACE, "Replace text after point, asking at each match", replace::query_replace),
    Command::new(replace::REPLACE_MATCH, "Replace this match and go to the next", replace::replace_match),
    Command::new(replace::SKIP_MATCH, "Leave this match and go to the next", replace::skip_match),
    Command::new(replace::REPLACE_ALL, "Replace this match and every one after it", replace::replace_all),
    Command::new(replace::QUIT, "Stop replacing, keeping what was replaced", replace::quit),
];

const BINDINGS: &[(&str, &str)] = &[("C-s", lines::SEARCH), ("M-%", replace::QUERY_REPLACE)];

impl Plugin for SearchPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.decorators.push(Rc::new(preview::decorate));
        registry.post_command_hooks.push(Rc::new(preview::sync));
        registry.post_command_hooks.push(Rc::new(replace::sync));
    }
}

#[cfg(test)]
mod tests;
