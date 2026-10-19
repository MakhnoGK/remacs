/// What a character is to word steps and word selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharClass {
    Word,
    Space,
    Punctuation,
}

impl CharClass {
    /// Identifier characters are words, so `snake_case` and `$name` are one, as in IntelliJ.
    pub fn of(character: char) -> Self {
        if character.is_alphanumeric() || character == '_' || character == '$' {
            Self::Word
        } else if character.is_whitespace() {
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
    fn identifier_characters_are_words() {
        for character in ['_', '$', 'é', '7'] {
            assert_eq!(CharClass::of(character), CharClass::Word);
        }
        assert_eq!(CharClass::of('\n'), CharClass::Space);
        assert_eq!(CharClass::of('.'), CharClass::Punctuation);
    }
}
