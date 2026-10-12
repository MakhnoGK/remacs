/// What a character is to word steps and word selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharClass {
    Word,
    Space,
    Punctuation,
}

impl CharClass {
    /// As word steps see it: `_` parts `snake_case`, as in Emacs.
    pub fn of(character: char) -> Self {
        if character.is_alphanumeric() {
            Self::Word
        } else {
            Self::not_word(character)
        }
    }

    /// As a double click sees it: identifier characters, so it takes `snake_case` whole.
    pub fn in_identifier(character: char) -> Self {
        if character.is_alphanumeric() || character == '_' {
            Self::Word
        } else {
            Self::not_word(character)
        }
    }

    fn not_word(character: char) -> Self {
        if character.is_whitespace() {
            Self::Space
        } else {
            Self::Punctuation
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underscore_is_a_word_only_in_identifiers() {
        assert_eq!(CharClass::of('_'), CharClass::Punctuation);
        assert_eq!(CharClass::in_identifier('_'), CharClass::Word);
        for of in [CharClass::of, CharClass::in_identifier] {
            assert_eq!(of('é'), CharClass::Word);
            assert_eq!(of('\n'), CharClass::Space);
            assert_eq!(of('.'), CharClass::Punctuation);
        }
    }
}
