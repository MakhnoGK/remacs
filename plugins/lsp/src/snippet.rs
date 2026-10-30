use std::iter::Peekable;
use std::str::Chars;

/// The text an LSP snippet inserts, placeholders keeping their defaults, and where its
/// first tab stop is: `step_by(${1:step})$0` is `step_by(step)` with point on `step`.
pub(crate) fn expand(snippet: &str) -> (String, Option<usize>) {
    let mut expansion = Expansion::default();
    expansion.read(&mut snippet.chars().peekable(), false);
    let first = expansion
        .stops
        .iter()
        .filter(|(number, _)| *number > 0)
        .min_by_key(|(number, _)| *number)
        .or_else(|| expansion.stops.first());
    let cursor = first.map(|&(_, offset)| offset);
    (expansion.text, cursor)
}

#[derive(Default)]
struct Expansion {
    text: String,
    /// Tab stop numbers and where they are in `text`.
    stops: Vec<(u32, usize)>,
}

impl Expansion {
    /// Reads up to the end, or up to the `}` closing a placeholder when `nested`.
    fn read(&mut self, chars: &mut Peekable<Chars>, nested: bool) {
        while let Some(character) = chars.next() {
            match character {
                '\\' => self.text.extend(chars.next()),
                '}' if nested => return,
                '$' => self.dollar(chars),
                _ => self.text.push(character),
            }
        }
    }

    fn dollar(&mut self, chars: &mut Peekable<Chars>) {
        match chars.peek() {
            Some(digit) if digit.is_ascii_digit() => {
                let number = number(chars);
                self.stops.push((number, self.text.len()));
            }
            Some('{') => {
                chars.next();
                self.braced(chars);
            }
            // Variables such as `$TM_FILENAME` are not known here and insert nothing.
            Some(&first) if first.is_ascii_alphabetic() || first == '_' => {
                while chars.next_if(|&next| next.is_ascii_alphanumeric() || next == '_').is_some() {
                }
            }
            _ => self.text.push('$'),
        }
    }

    /// `${1}`, `${1:default}`, `${1|one,two|}`, `${NAME}` or `${NAME:default}`.
    fn braced(&mut self, chars: &mut Peekable<Chars>) {
        if chars.peek().is_some_and(char::is_ascii_digit) {
            let number = number(chars);
            self.stops.push((number, self.text.len()));
        } else {
            while chars.next_if(|&next| next != ':' && next != '}').is_some() {}
        }
        match chars.next() {
            Some(':') => self.read(chars, true),
            Some('|') => self.choice(chars),
            _ => {}
        }
    }

    /// The first of the choices.
    fn choice(&mut self, chars: &mut Peekable<Chars>) {
        let mut first = true;
        while let Some(character) = chars.next() {
            match character {
                '|' => {
                    chars.next_if_eq(&'}');
                    return;
                }
                ',' => first = false,
                '\\' if first => self.text.extend(chars.next()),
                _ if first => self.text.push(character),
                _ => {}
            }
        }
    }
}

fn number(chars: &mut Peekable<Chars>) -> u32 {
    let mut number = 0u32;
    while let Some(digit) = chars.next_if(char::is_ascii_digit) {
        number = number.saturating_mul(10).saturating_add(digit.to_digit(10).unwrap_or(0));
    }
    number
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_keep_their_text_and_point_goes_to_the_first_stop() {
        assert_eq!(expand("step_by(${1:step})$0"), ("step_by(step)".into(), Some(8)));
        assert_eq!(expand("len()$0"), ("len()".into(), Some(5)));
        assert_eq!(expand("if ${2:b} {\n\t$1\n}"), ("if b {\n\t\n}".into(), Some(8)));
        assert_eq!(
            expand("${1:outer ${2:inner}} \\$x ${3|a,b|} $TM_FILENAME"),
            ("outer inner $x a ".into(), Some(0))
        );
        assert_eq!(expand("plain"), ("plain".into(), None));
    }
}
