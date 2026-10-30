use gpui::{Action, App, KeyBinding, Menu, MenuItem, SharedString};
use remacs_core::{Editor, Key, KeyCode, describe_sequence};

use crate::key_input::NAMED_KEYS;

/// A menu item's command, run by the editor view.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = remacs, no_json)]
pub struct RunCommand {
    pub name: SharedString,
}

/// Menu key equivalents only show bindings: gpui never matches this context, so the
/// keys stay with the editor's keymaps.
const DISPLAY_ONLY: &str = "RemacsMenu";

/// Fills the menu bar with the plugins' menus.
pub fn set_menus(editor: &Editor, cx: &mut App) {
    let mut bindings = Vec::new();
    let menus = shown_menus(editor)
        .into_iter()
        .map(|menu| {
            let mut items = Vec::new();
            for (index, group) in menu.groups.into_iter().enumerate() {
                if index > 0 {
                    items.push(MenuItem::separator());
                }
                for item in group {
                    let action = RunCommand { name: item.command.into() };
                    if let Some(keystroke) = item.key_equivalent {
                        bindings.push(KeyBinding::new(
                            &keystroke,
                            action.clone(),
                            Some(DISPLAY_ONLY),
                        ));
                    }
                    items.push(MenuItem::action(item.title, action));
                }
            }
            Menu { name: menu.title.into(), items }
        })
        .collect();
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
    cx.set_menus(menus);
}

struct ShownMenu {
    title: &'static str,
    groups: Vec<Vec<ShownItem>>,
}

/// An item with the keys bound to its command: a single key as macOS shows it, at the
/// item's right, else a sequence after the title, as Emacs does on macOS.
struct ShownItem {
    title: String,
    command: &'static str,
    /// In gpui's notation.
    key_equivalent: Option<String>,
}

fn shown_menus(editor: &Editor) -> Vec<ShownMenu> {
    editor
        .menu_bar()
        .into_iter()
        .map(|menu| ShownMenu {
            title: menu.title,
            groups: menu
                .groups
                .iter()
                .map(|group| group.iter().map(|item| shown_item(editor, item)).collect())
                .collect(),
        })
        .collect()
}

fn shown_item(editor: &Editor, item: &remacs_core::MenuItem) -> ShownItem {
    let title = item.title_in(editor);
    let (title, key_equivalent) = match shown_keys(editor, item.command) {
        Some(Shown::Native(keystroke)) => (title.to_owned(), Some(keystroke)),
        Some(Shown::InTitle(keys)) => (format!("{title} ({keys})"), None),
        None => (title.to_owned(), None),
    };
    ShownItem { title, command: item.command, key_equivalent }
}

/// Whether a menu item may be chosen: its command can run now.
pub fn enabled(editor: &Editor, action: &dyn Action) -> bool {
    action
        .as_any()
        .downcast_ref::<RunCommand>()
        .is_none_or(|run| editor.menu_command_enabled(&run.name))
}

enum Shown {
    /// A gpui keystroke, shown by macOS at the item's right.
    Native(String),
    InTitle(String),
}

/// The binding of `command` to show: a single key, ⌘ ones first, else the shortest
/// sequence.
fn shown_keys(editor: &Editor, command: &str) -> Option<Shown> {
    let mut sequences: Vec<&[Key]> = editor.keymap().sequences_for(command).collect();
    sequences.sort_by_key(|keys| {
        let native = keys.len() == 1 && to_keystroke(keys[0]).is_some();
        let command_key = keys.first().is_some_and(|key| key.modifiers.super_key);
        (!native, !command_key, keys.len(), describe_sequence(keys))
    });
    let keys = sequences.first()?;
    let native = match keys {
        [key] => to_keystroke(*key),
        _ => None,
    };
    Some(native.map_or_else(|| Shown::InTitle(describe_sequence(keys)), Shown::Native))
}

/// gpui's notation of a key macOS can show as a menu key equivalent; chords only, so that
/// no item shows a key that types.
fn to_keystroke(key: Key) -> Option<String> {
    let modifiers = key.modifiers;
    let name = match key.code {
        KeyCode::Char(character) if character.is_ascii_uppercase() => {
            format!("shift-{}", character.to_ascii_lowercase())
        }
        KeyCode::Char(character) if !character.is_whitespace() && character != '-' => {
            character.to_string()
        }
        KeyCode::Function(number) => format!("f{number}"),
        // macOS menus have no key equivalent for them, nor for `-` and whitespace but space.
        KeyCode::Return | KeyCode::Tab => return None,
        code => NAMED_KEYS.iter().find(|&&(_, named)| named == code)?.0.to_owned(),
    };
    let chord = modifiers.control || modifiers.meta || modifiers.super_key;
    if !chord && !matches!(key.code, KeyCode::Function(_)) {
        return None;
    }
    let prefixes = [
        (modifiers.control, "ctrl-"),
        (modifiers.meta, "alt-"),
        (modifiers.shift, "shift-"),
        (modifiers.super_key, "cmd-"),
    ];
    let mut keystroke: String =
        prefixes.into_iter().filter_map(|(held, prefix)| held.then_some(prefix)).collect();
    keystroke.push_str(&name);
    Some(keystroke)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keystroke(notation: &str) -> Option<String> {
        to_keystroke(Key::parse(notation).unwrap())
    }

    #[test]
    fn chords_and_function_keys_become_key_equivalents() {
        assert_eq!(keystroke("s-s").as_deref(), Some("cmd-s"));
        assert_eq!(keystroke("s-S").as_deref(), Some("cmd-shift-s"));
        assert_eq!(keystroke("C-s-f").as_deref(), Some("ctrl-cmd-f"));
        assert_eq!(keystroke("M-s-h").as_deref(), Some("alt-cmd-h"));
        assert_eq!(keystroke("s-,").as_deref(), Some("cmd-,"));
        assert_eq!(keystroke("S-<f9>").as_deref(), Some("shift-f9"));
        assert_eq!(keystroke("s-<up>").as_deref(), Some("cmd-up"));
        assert_eq!(keystroke("<f8>").as_deref(), Some("f8"));
    }

    #[test]
    fn every_item_runs_a_command_and_shows_its_keys() {
        let editor = Editor::new(crate::BUILTIN_PLUGINS);
        let menus = shown_menus(&editor);
        let titles: Vec<&str> = menus.iter().map(|menu| menu.title).collect();
        assert_eq!(
            titles,
            ["Remacs", "File", "Edit", "View", "Go", "Git", "Run", "Window", "Help"]
        );
        let items: Vec<&ShownItem> =
            menus.iter().flat_map(|menu| menu.groups.iter().flatten()).collect();
        for item in &items {
            assert!(
                editor.commands().get(item.command).is_some(),
                "{} runs no command {}",
                item.title,
                item.command
            );
        }
        let shown = |command: &str| {
            items
                .iter()
                .find(|item| item.command == command)
                .map(|item| (item.title.as_str(), item.key_equivalent.as_deref()))
                .unwrap()
        };
        assert_eq!(shown("save-buffer"), ("Save", Some("cmd-s")));
        assert_eq!(shown("save-some-buffers"), ("Save All… (C-x s)", None));
        assert_eq!(shown("open-settings"), ("Settings…", Some("cmd-,")));
        assert_eq!(shown("debug-continue"), ("Continue", Some("f9")));
        assert_eq!(shown("toggle-frame-maximized"), ("Zoom", None));
    }

    #[test]
    fn typing_keys_and_keys_menus_cannot_show_stay_in_the_title() {
        assert_eq!(keystroke("q"), None);
        assert_eq!(keystroke("<up>"), None);
        assert_eq!(keystroke("s-RET"), None);
        assert_eq!(keystroke("C--"), None);
    }
}
