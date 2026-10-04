use std::collections::{HashMap, HashSet};

use crate::key::{Key, parse_sequence};

#[derive(Clone, Copy)]
pub enum Binding {
    Command(&'static str),
    Prefix,
    Unbound,
}

#[derive(Default)]
pub struct Keymap {
    commands: HashMap<Vec<Key>, &'static str>,
    prefixes: HashSet<Vec<Key>>,
}

impl Keymap {
    /// Binds a key sequence in Emacs notation, e.g. `"C-x C-s"`.
    pub fn bind(&mut self, notation: &str, command: &'static str) {
        let keys =
            parse_sequence(notation).unwrap_or_else(|| panic!("invalid key sequence {notation:?}"));
        for prefix_len in 1..keys.len() {
            self.prefixes.insert(keys[..prefix_len].to_vec());
        }
        self.commands.insert(keys, command);
    }

    pub fn lookup(&self, keys: &[Key]) -> Binding {
        if let Some(&command) = self.commands.get(keys) {
            Binding::Command(command)
        } else if self.prefixes.contains(keys) {
            Binding::Prefix
        } else {
            Binding::Unbound
        }
    }

    /// Looks `keys` up in active keymaps, highest priority first; the first that binds
    /// them wins, as in Emacs.
    pub fn lookup_in(keymaps: &[&Keymap], keys: &[Key]) -> Binding {
        keymaps
            .iter()
            .map(|keymap| keymap.lookup(keys))
            .find(|binding| !matches!(binding, Binding::Unbound))
            .unwrap_or(Binding::Unbound)
    }

    pub fn sequences_for<'a>(&'a self, command: &'a str) -> impl Iterator<Item = &'a [Key]> {
        self.commands
            .iter()
            .filter(move |(_, bound)| **bound == command)
            .map(|(keys, _)| keys.as_slice())
    }
}
