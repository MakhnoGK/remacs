use std::collections::BTreeMap;

use remacs_config::Section;
use serde::Deserialize;

use crate::command::Commands;
use crate::keymap::Keymap;

/// `keys.toml`: key sequences in Emacs notation bound to commands over the built-in
/// bindings, e.g. `"C-r" = "run"`; an empty command unbinds the keys.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct KeysSettings {
    bindings: BTreeMap<String, String>,
}

impl Section for KeysSettings {
    const NAME: &str = "keys";
}

impl KeysSettings {
    /// `defaults` with these bindings over them, and what was wrong with any of them.
    pub(crate) fn apply(&self, defaults: &Keymap, commands: &Commands) -> (Keymap, Vec<String>) {
        let mut keymap = defaults.clone();
        let mut errors = Vec::new();
        for (keys, command) in &self.bindings {
            let bound = if command.is_empty() {
                keymap.unbind(keys)
            } else if let Some(command) = commands.get(command) {
                keymap.try_bind(keys, command.name)
            } else {
                errors.push(format!("keys.toml: no command {command}"));
                continue;
            };
            if !bound {
                errors.push(format!("keys.toml: not a key sequence: {keys}"));
            }
        }
        (keymap, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::key::parse_sequence;
    use crate::keymap::Binding;

    fn bound(keymap: &Keymap, keys: &str) -> Option<&'static str> {
        match keymap.lookup(&parse_sequence(keys).unwrap()) {
            Binding::Command(command) => Some(command),
            _ => None,
        }
    }

    #[test]
    fn the_default_file_reads_as_the_defaults() {
        remacs_config::assert_default_file::<KeysSettings>();
    }

    #[test]
    fn bindings_go_over_the_defaults_and_report_mistakes() {
        let mut commands = Commands::default();
        commands.add(Command::new("run", "", |_| Ok(())));
        commands.add(Command::new("stop", "", |_| Ok(())));
        let mut defaults = Keymap::default();
        defaults.bind("C-r", "run");
        defaults.bind("s-<f2>", "stop");
        let settings: KeysSettings = toml::from_str(
            "[bindings]\n\"<f5>\" = \"run\"\n\"C-r\" = \"\"\n\"C-q\" = \"nothing\"\n\"C-\" = \"stop\"\n",
        )
        .unwrap();
        let (keymap, errors) = settings.apply(&defaults, &commands);
        assert_eq!(bound(&keymap, "<f5>"), Some("run"));
        assert_eq!(bound(&keymap, "C-r"), None);
        assert_eq!(bound(&keymap, "s-<f2>"), Some("stop"));
        assert_eq!(
            errors,
            [
                "keys.toml: not a key sequence: C-",
                "keys.toml: no command nothing"
            ]
        );
    }
}
