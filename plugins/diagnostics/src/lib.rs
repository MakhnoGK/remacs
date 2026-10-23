//! Built-in plugin: problems that checkers such as language servers report for a buffer,
//! underlined, counted in the status pill, listed and shown at point, like Emacs flymake.

mod bubble;
mod buffer_diagnostics;
mod problems;

use std::rc::Rc;

use remacs_core::{Command, MenuItem, Plugin, Registry, visiting_file};

pub use buffer_diagnostics::{Diagnostic, Severity, clear, publish};

pub struct DiagnosticsPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(problems::LIST_PROBLEMS, "Show the buffer's problems, or hide them", problems::list_problems),
    Command::new("next-problem", "Go to the next problem in the buffer", problems::next_problem),
    Command::new("previous-problem", "Go to the previous problem in the buffer", problems::previous_problem),
    Command::new("next-error", "Go to the next location of the last compile output, else the buffer's next error or warning", problems::next_error),
    Command::new("previous-error", "Go to the previous location of the last compile output, else the buffer's previous error or warning", problems::previous_error),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-c ! l", problems::LIST_PROBLEMS),
    ("M-n", "next-problem"),
    ("M-p", "previous-problem"),
    ("M-g n", "next-error"),
    ("M-g M-n", "next-error"),
    ("C-x `", "next-error"),
    ("M-g p", "previous-error"),
    ("M-g M-p", "previous-error"),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("View", "panels", "Problems", problems::LIST_PROBLEMS),
    MenuItem::new("Go", "problems", "Next Problem", "next-problem").enabled_when(visiting_file),
    MenuItem::new("Go", "problems", "Previous Problem", "previous-problem").enabled_when(visiting_file),
    MenuItem::new("Go", "problems", "Next Error", "next-error").enabled_when(problems::has_errors),
    MenuItem::new("Go", "problems", "Previous Error", "previous-error").enabled_when(problems::has_errors),
];

impl Plugin for DiagnosticsPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
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
