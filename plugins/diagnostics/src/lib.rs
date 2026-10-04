//! Built-in plugin: problems that checkers such as language servers report for a buffer,
//! underlined, counted in the status pill and listed, like Emacs flymake.

mod buffer_diagnostics;
mod problems;

use std::rc::Rc;

use remacs_core::{Command, Plugin, Registry};

pub use buffer_diagnostics::{Diagnostic, Severity, clear, publish};

pub struct DiagnosticsPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(problems::LIST_PROBLEMS, "Show the buffer's problems, or hide them", problems::list_problems),
    Command::new("next-problem", "Go to the next problem in the buffer", problems::next_problem),
    Command::new("previous-problem", "Go to the previous problem in the buffer", problems::previous_problem),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-c ! l", problems::LIST_PROBLEMS),
    ("M-n", "next-problem"),
    ("M-p", "previous-problem"),
];

impl Plugin for DiagnosticsPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry
            .post_command_hooks
            .push(Rc::new(buffer_diagnostics::rebase));
        registry
            .decorators
            .push(Rc::new(buffer_diagnostics::underline));
        registry.status_providers.push(Rc::new(problems::status));
    }
}

#[cfg(test)]
mod tests;
