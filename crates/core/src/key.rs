use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub control: bool,
    pub meta: bool,
    /// Only for named keys; for characters shift is folded into the character.
    pub shift: bool,
    pub super_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),
    Return,
    Tab,
    Backspace,
    Delete,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

/// Emacs names of non-character keys.
const KEY_NAMES: [(KeyCode, &str); 14] = [
    (KeyCode::Char(' '), "SPC"),
    (KeyCode::Return, "RET"),
    (KeyCode::Tab, "TAB"),
    (KeyCode::Backspace, "DEL"),
    (KeyCode::Delete, "<deletechar>"),
    (KeyCode::Escape, "ESC"),
    (KeyCode::Up, "<up>"),
    (KeyCode::Down, "<down>"),
    (KeyCode::Left, "<left>"),
    (KeyCode::Right, "<right>"),
    (KeyCode::Home, "<home>"),
    (KeyCode::End, "<end>"),
    (KeyCode::PageUp, "<prior>"),
    (KeyCode::PageDown, "<next>"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub modifiers: Modifiers,
    pub code: KeyCode,
}

impl Key {
    pub fn new(modifiers: Modifiers, code: KeyCode) -> Self {
        Self { modifiers, code }
    }

    /// Parses Emacs notation: `a`, `C-x`, `M-<`, `s-z`, `RET`, `S-<up>`.
    pub fn parse(notation: &str) -> Option<Self> {
        let mut modifiers = Modifiers::default();
        let mut rest = notation;
        while rest.len() > 2 && rest.as_bytes()[1] == b'-' {
            match rest.as_bytes()[0] {
                b'C' => modifiers.control = true,
                b'M' => modifiers.meta = true,
                b'S' => modifiers.shift = true,
                b's' => modifiers.super_key = true,
                _ => break,
            }
            rest = &rest[2..];
        }
        let code = match KEY_NAMES.iter().find(|(_, name)| *name == rest) {
            Some((code, _)) => *code,
            None => {
                let mut chars = rest.chars();
                let character = chars.next()?;
                chars.next().is_none().then_some(KeyCode::Char(character))?
            }
        };
        Some(Self { modifiers, code })
    }

    /// The character this key types, if it is not a command chord.
    pub fn printable_char(&self) -> Option<char> {
        let Modifiers {
            control,
            meta,
            super_key,
            ..
        } = self.modifiers;
        match self.code {
            KeyCode::Char(character) if !control && !meta && !super_key => Some(character),
            _ => None,
        }
    }
}

pub fn parse_sequence(notation: &str) -> Option<Vec<Key>> {
    notation.split_whitespace().map(Key::parse).collect()
}

pub fn describe_sequence(keys: &[Key]) -> String {
    keys.iter()
        .map(Key::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let prefixes = [
            (self.modifiers.control, "C-"),
            (self.modifiers.meta, "M-"),
            (self.modifiers.shift, "S-"),
            (self.modifiers.super_key, "s-"),
        ];
        for (active, prefix) in prefixes {
            if active {
                formatter.write_str(prefix)?;
            }
        }
        match KEY_NAMES.iter().find(|(code, _)| *code == self.code) {
            Some((_, name)) => formatter.write_str(name),
            None => match self.code {
                KeyCode::Char(character) => write!(formatter, "{character}"),
                _ => unreachable!("every named key has a name"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_describes_emacs_notation() {
        for notation in [
            "a", "C-x", "M-<", "C-M-%", "s-z", "RET", "S-<up>", "C--", "SPC",
        ] {
            assert_eq!(Key::parse(notation).unwrap().to_string(), notation);
        }
        assert_eq!(
            describe_sequence(&parse_sequence("C-x C-s").unwrap()),
            "C-x C-s"
        );
        assert_eq!(Key::parse("C-xy"), None);
    }

    #[test]
    fn only_unmodified_characters_are_printable() {
        assert_eq!(Key::parse("ж").unwrap().printable_char(), Some('ж'));
        assert_eq!(Key::parse("C-f").unwrap().printable_char(), None);
        assert_eq!(Key::parse("RET").unwrap().printable_char(), None);
    }
}
