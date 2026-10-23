//! Built-in plugin: searching the buffer, its matching lines listed in a prompt like the
//! other searches, like Emacs `consult-line`, and replacing in it, like `query-replace`;
//! searching the whole project's text, like `consult-ripgrep`, and replacing in it.

mod edit_results;
mod lines;
mod preview;
mod project;
mod project_preview;
mod project_replace;
mod query;
mod replace;
mod scan;
mod words;

use std::rc::Rc;

use remacs_core::{Command, Keymap, MenuItem, MinorKeymap, Plugin, Registry};

pub struct SearchPlugin;

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(lines::SEARCH, "Search the buffer, listing the lines that match", lines::search),
    Command::new(lines::SEARCH_REGEXP, "Search the buffer for a regexp, listing the lines that match", lines::search_regexp),
    Command::new(replace::QUERY_REPLACE, "Replace text after point, asking at each match", replace::query_replace),
    Command::new(replace::QUERY_REPLACE_REGEXP, "Replace a regexp after point, asking at each match", replace::query_replace_regexp),
    Command::new(replace::REPLACE_MATCH, "Replace this match and go to the next", replace::replace_match),
    Command::new(replace::SKIP_MATCH, "Leave this match and go to the next", replace::skip_match),
    Command::new(replace::REPLACE_ALL, "Replace this match and every one after it", replace::replace_all),
    Command::new(replace::QUIT, "Stop replacing, keeping what was replaced", replace::quit),
    Command::new(project::PROJECT_SEARCH, "Search the project's text, listing the lines that match", project::project_search),
    Command::new(project_replace::PROJECT_QUERY_REPLACE, "Replace text in the whole project, asking at each match", project_replace::project_query_replace),
    Command::new(project_replace::REPLACE_MATCH, "Replace this match and go to the next", project_replace::replace_match),
    Command::new(project_replace::SKIP_MATCH, "Leave this match and go to the next", project_replace::skip_match),
    Command::new(project_replace::REPLACE_FILE, "Replace this match and the rest of the file", project_replace::replace_file),
    Command::new(project_replace::REPLACE_ALL, "Replace this match and every one after it in every file", project_replace::replace_all),
    Command::new(project_replace::QUIT, "Stop replacing, keeping what was replaced", project_replace::quit),
    Command::new(edit_results::EDIT_SEARCH_RESULTS, "List the lines the project search found in a buffer to edit them", edit_results::edit_search_results),
    Command::new(edit_results::FINISH, "Write the edited search results to their files", edit_results::finish),
    Command::new(edit_results::ABORT, "Discard the edited search results", edit_results::abort),
];

const BINDINGS: &[(&str, &str)] = &[
    ("C-s", lines::SEARCH),
    ("C-M-s", lines::SEARCH_REGEXP),
    ("M-%", replace::QUERY_REPLACE),
    ("C-M-%", replace::QUERY_REPLACE_REGEXP),
    ("M-s g", project::PROJECT_SEARCH),
    ("s-F", project::PROJECT_SEARCH),
    ("M-s %", project_replace::PROJECT_QUERY_REPLACE),
    ("s-H", project_replace::PROJECT_QUERY_REPLACE),
];

/// While the project search lists its lines.
const SEARCH_PROMPT_BINDINGS: &[(&str, &str)] = &[("C-x C-q", edit_results::EDIT_SEARCH_RESULTS)];

/// Over the editing keys, in the buffer of search results being edited.
const EDIT_RESULTS_BINDINGS: &[(&str, &str)] = &[
    ("C-c C-c", edit_results::FINISH),
    ("C-x C-s", edit_results::FINISH),
    ("C-c C-k", edit_results::ABORT),
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new("Edit", "find", "Search…", lines::SEARCH),
    MenuItem::new("Edit", "find", "Search Regexp…", lines::SEARCH_REGEXP),
    MenuItem::new("Edit", "find", "Replace…", replace::QUERY_REPLACE),
    MenuItem::new("Edit", "find", "Replace Regexp…", replace::QUERY_REPLACE_REGEXP),
    MenuItem::new("Edit", "find", "Search in Project…", project::PROJECT_SEARCH),
    MenuItem::new("Edit", "find", "Replace in Project…", project_replace::PROJECT_QUERY_REPLACE),
    MenuItem::new("Edit", "find", "Edit Search Results", edit_results::EDIT_SEARCH_RESULTS).enabled_when(project::is_open),
];

impl Plugin for SearchPlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menu_items.extend_from_slice(MENU_ITEMS);
        registry.minor_keymaps.push(MinorKeymap {
            active: project::is_open,
            keymap: keymap(SEARCH_PROMPT_BINDINGS),
        });
        registry.minor_keymaps.push(MinorKeymap {
            active: |editor| editor.buffer_has_keys() && edit_results::is_editing(editor),
            keymap: keymap(EDIT_RESULTS_BINDINGS),
        });
        registry.decorators.push(Rc::new(preview::decorate));
        registry.decorators.push(Rc::new(project_preview::decorate));
        registry.post_command_hooks.push(Rc::new(preview::sync));
        registry.post_command_hooks.push(Rc::new(replace::sync));
        registry.post_command_hooks.push(Rc::new(project::update));
        registry
            .post_command_hooks
            .push(Rc::new(project_replace::update));
    }
}

fn keymap(bindings: &[(&str, &'static str)]) -> Keymap {
    let mut keymap = Keymap::default();
    for &(keys, command) in bindings {
        keymap.bind(keys, command);
    }
    keymap
}

#[cfg(test)]
mod tests;
