//! Built-in plugin: problems that checkers such as language servers report for a buffer,
//! underlined, counted in the status pill, listed and shown at point, like Emacs flymake.

mod bubble;
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
    Command::new("next-error", "Go to the next error or warning in the buffer", problems::next_error),
    Command::new("previous-error", "Go to the previous error or warning in the buffer", problems::previous_error),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-c ! l", problems::LIST_PROBLEMS),
    ("M-n", "next-problem"),
    ("M-p", "previous-problem"),
    ("M-g n", "next-error"),
    ("M-g p", "previous-error"),
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
        registry.post_command_hooks.push(Rc::new(bubble::update));
        registry.popup_providers.push(Rc::new(bubble::popup));
        registry
            .decorators
            .push(Rc::new(buffer_diagnostics::underline));
        registry.status_providers.push(Rc::new(problems::status));
    }
}

#[cfg(test)]
mod tests;
