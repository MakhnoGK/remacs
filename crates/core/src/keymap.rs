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
    /// Binds a key sequence in Emacs notation, e.g. `"C-x C-s"`. Built-in bindings must not
    /// clash: keys bound to another command, or a prefix of keys bound or keys a prefix of.
    pub fn bind(&mut self, notation: &str, command: &'static str) {
        let keys = parse_sequence(notation).unwrap_or_default();
        let clash = match self.lookup(&keys) {
            Binding::Command(bound) => (bound != command).then_some(bound),
            Binding::Prefix => Some("a prefix"),
            Binding::Unbound => {
                (1..keys.len()).find_map(|len| self.commands.get(&keys[..len]).copied())
            }
        };
        if let Some(bound) = clash {
            panic!("{notation:?} is bound to {command} and {bound}");
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_keys_again_to_the_same_command_is_no_clash() {
        let mut keymap = Keymap::default();
        keymap.bind("C-x C-s", "save");
        keymap.bind("C-x C-s", "save");
        keymap.bind("C-x C-f", "find");
    }

    #[test]
    #[should_panic(expected = "\"C-r\" is bound to search and run")]
    fn keys_bound_to_two_commands_clash() {
        let mut keymap = Keymap::default();
        keymap.bind("C-r", "run");
        keymap.bind("C-r", "search");
    }

    #[test]
    #[should_panic(expected = "\"C-x r\" is bound to rename and a prefix")]
    fn keys_that_are_a_prefix_clash() {
        let mut keymap = Keymap::default();
        keymap.bind("C-x r t", "string-rectangle");
        keymap.bind("C-x r", "rename");
    }

    #[test]
    #[should_panic(expected = "\"C-x r t\" is bound to string-rectangle and rename")]
    fn keys_under_a_bound_prefix_clash() {
        let mut keymap = Keymap::default();
        keymap.bind("C-x r", "rename");
        keymap.bind("C-x r t", "string-rectangle");
    }
}
