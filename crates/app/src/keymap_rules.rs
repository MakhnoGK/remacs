//! The built-in keymaps keep Emacs's conventions (ADR 0079); `Keymap::bind` refuses
//! clashes within one keymap as the plugins register.

use remacs_core::{Key, Registry};

#[test]
fn c_c_and_a_letter_is_left_to_the_user() {
    let mut registry = Registry::default();
    for plugin in crate::BUILTIN_PLUGINS {
        plugin.register(&mut registry);
    }
    let keymaps = [
        &registry.keymap,
        &registry.minibuffer_keymap,
        &registry.message_keymap,
    ]
    .into_iter()
    .chain(registry.minor_keymaps.iter().map(|minor| &minor.keymap));
    let prefix = [Key::parse("C-c").unwrap()];
    let taken: Vec<String> = keymaps
        .flat_map(|keymap| {
            keymap
                .continuations(&prefix)
                .map(|(key, _)| key.to_string())
        })
        .filter(|key| key.len() == 1 && key.chars().all(|char| char.is_ascii_alphabetic()))
        .map(|key| format!("C-c {key}"))
        .collect();
    assert!(taken.is_empty(), "plugins bind {taken:?}");
}
