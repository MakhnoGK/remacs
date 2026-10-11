use std::collections::{HashMap, HashSet};

use crate::key::{Key, parse_sequence};

#[derive(Clone, Copy)]
pub enum Binding {
    Command(&'static str),
    Prefix,
    Unbound,
}

#[derive(Clone, Default)]
pub struct Keymap {
    commands: HashMap<Vec<Key>, &'static str>,
    prefixes: HashSet<Vec<Key>>,
}

impl Keymap {
    /// Binds a key sequence in Emacs notation, e.g. `"C-x C-s"`.
    pub fn bind(&mut self, notation: &str, command: &'static str) {
        if !self.try_bind(notation, command) {
            panic!("invalid key sequence {notation:?}");
        }
    }

    /// Like [`Keymap::bind`], for notation that may be wrong, e.g. from a settings file;
    /// false when it is.
    pub fn try_bind(&mut self, notation: &str, command: &'static str) -> bool {
        let Some(keys) = parse_sequence(notation).filter(|keys| !keys.is_empty()) else {
            return false;
        };
        for prefix_len in 1..keys.len() {
            self.prefixes.insert(keys[..prefix_len].to_vec());
        }
        self.commands.insert(keys, command);
        true
    }

    /// Leaves a key sequence unbound; false when the notation is wrong.
    pub fn unbind(&mut self, notation: &str) -> bool {
        let Some(keys) = parse_sequence(notation) else {
            return false;
        };
        self.commands.remove(&keys);
        true
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

    /// The keys that may follow `prefix`, each with what the sequence then is.
    pub fn continuations<'a>(
        &'a self,
        prefix: &'a [Key],
    ) -> impl Iterator<Item = (Key, Binding)> + 'a {
        let next = move |keys: &Vec<Key>| {
            (keys.len() == prefix.len() + 1 && keys.starts_with(prefix)).then(|| keys[prefix.len()])
        };
        let commands = self
            .commands
            .iter()
            .filter_map(move |(keys, &command)| Some((next(keys)?, Binding::Command(command))));
        let prefixes = self
            .prefixes
            .iter()
            .filter_map(move |keys| Some((next(keys)?, Binding::Prefix)));
        commands.chain(prefixes)
    }

    pub fn sequences_for<'a>(&'a self, command: &'a str) -> impl Iterator<Item = &'a [Key]> {
        self.commands
            .iter()
            .filter(move |(_, bound)| **bound == command)
            .map(|(keys, _)| keys.as_slice())
    }
}
