//! Breaks long lines of code in documentation where a reader expects them: after a comma
//! or an opening bracket, before an arrow, a return type's `:`, `where`, `=` or an operator
//! joining types. A break inside fewer brackets wins over one that fits more on the line.

use std::cmp::Reverse;

/// Beyond the line's own indent, of the lines a long line was broken into.
const CONTINUATION_INDENT: usize = 4;
/// Words a line may break before, the space ahead of them dropped.
const ARROWS: &[&str] = &["-> ", "=> ", "where "];
/// Like [`ARROWS`], but they bind tighter than the commas around them, as if a bracket
/// deeper: `a: B | C, d: E` breaks at the comma first.
const OPERATORS: &[&str] = &["= ", "| ", "& ", "+ "];

/// `code` with each line longer than `columns` broken into lines that fit; a part with
/// nowhere to break stays longer.
pub fn wrap_code(code: &str, columns: usize) -> String {
    code.lines()
        .map(|line| wrap_line(line, columns))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The line ends at `end` and the next starts at `next`, `depth` brackets in.
struct Break {
    end: usize,
    next: usize,
    depth: usize,
}

fn wrap_line(line: &str, columns: usize) -> String {
    let indent = line.len() - line.trim_start().len();
    let continuation = " ".repeat(width(&line[..indent]) + CONTINUATION_INDENT);
    let breaks = breaks(line);
    let mut wrapped = String::with_capacity(line.len());
    let mut start = 0;
    loop {
        let lead = if start == 0 {
            0
        } else {
            wrapped.push('\n');
            wrapped.push_str(&continuation);
            continuation.len()
        };
        if lead + width(&line[start..]) <= columns {
            wrapped.push_str(&line[start..]);
            return wrapped;
        }
        let mut later = breaks.iter().filter(|candidate| candidate.end > start);
        let chosen = later
            .clone()
            .filter(|candidate| lead + width(&line[start..candidate.end]) <= columns)
            .min_by_key(|candidate| (candidate.depth, Reverse(candidate.end)))
            .or_else(|| later.next());
        let Some(chosen) = chosen else {
            wrapped.push_str(&line[start..]);
            return wrapped;
        };
        wrapped.push_str(&line[start..chosen.end]);
        start = chosen.next;
    }
}

fn breaks(line: &str) -> Vec<Break> {
    let mut breaks = Vec::new();
    let mut push = |end, next, depth| breaks.push(Break { end, next, depth });
    let mut depth = 0usize;
    let mut previous = None;
    for (index, character) in line.char_indices() {
        let after = index + character.len_utf8();
        let rest = &line[after..];
        let starts = |words: &[&str]| words.iter().any(|word| rest.starts_with(word));
        match character {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            '>' if !matches!(previous, Some('-' | '=')) => depth = depth.saturating_sub(1),
            _ => {}
        }
        match character {
            '(' if !rest.is_empty() && !rest.starts_with(')') => push(after, after, depth),
            ',' | ';' | '{' if rest.starts_with(' ') => push(after, after + 1, depth),
            ':' if previous == Some(')') && rest.starts_with(' ') => push(index, index, depth),
            ' ' if index > 0 && (starts(ARROWS) || starts(OPERATORS)) => {
                push(index, after, depth + usize::from(starts(OPERATORS)))
            }
            _ => {}
        }
        previous = Some(character);
    }
    breaks
}

fn width(text: &str) -> usize {
    text.chars().count()
}

/// Where byte `offset` of `code` went in `wrapped`, which [`wrap_code`] made of it by
/// adding line ends and indents and dropping spaces at breaks.
pub fn wrapped_offset(code: &str, wrapped: &str, offset: usize) -> usize {
    let mut position = 0;
    for character in code[..offset].chars() {
        let rest = &wrapped[position..];
        if rest.starts_with(character) || character != ' ' {
            // Past the line end and indent a break put before it.
            position += rest
                .find(character)
                .map_or(rest.len(), |skipped| skipped + character.len_utf8());
        }
    }
    match code[offset..].chars().next() {
        Some(next) if next != ' ' => position + wrapped[position..].find(next).unwrap_or(0),
        _ => position,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_lines_stay_as_they_are() {
        let code = "pub fn len(&self) -> usize\nwhere\n    T: Copy,";
        assert_eq!(wrap_code(code, 40), code);
    }

    #[test]
    fn rust_breaks_after_a_comma_and_before_the_arrow() {
        let code =
            "pub fn with_capacity_and_hasher(capacity: usize, hasher: S) -> HashMap<K, V, S>";
        assert_eq!(
            wrap_code(code, 50),
            "pub fn with_capacity_and_hasher(capacity: usize,\n    hasher: S) -> HashMap<K, V, S>"
        );
        assert_eq!(
            wrap_code(code, 40),
            "pub fn with_capacity_and_hasher(\n    capacity: usize, hasher: S)\n    -> HashMap<K, V, S>"
        );
    }

    #[test]
    fn offsets_follow_their_characters_past_breaks() {
        let code =
            "pub fn with_capacity_and_hasher(capacity: usize, hasher: S) -> HashMap<K, V, S>";
        let wrapped = wrap_code(code, 40);
        let at = |part: &str| {
            let start = code.find(part).unwrap();
            wrapped_offset(code, &wrapped, start)
                ..wrapped_offset(code, &wrapped, start + part.len())
        };
        assert_eq!(&wrapped[at("capacity: usize")], "capacity: usize");
        assert_eq!(&wrapped[at("hasher: S")], "hasher: S");
        assert_eq!(&wrapped[at("-> HashMap")], "-> HashMap");
    }

    #[test]
    fn typescript_breaks_at_commas_before_unions_and_then_before_the_return_type() {
        let code = "String.localeCompare(that: string, locales?: string | string[], options?: Intl.CollatorOptions): number";
        assert_eq!(
            wrap_code(code, 60),
            "String.localeCompare(that: string,\n    locales?: string | string[],\n    options?: Intl.CollatorOptions): number"
        );
        assert_eq!(
            wrap_code(code, 40),
            "String.localeCompare(that: string,\n    locales?: string | string[],\n    options?: Intl.CollatorOptions)\n    : number"
        );
    }

    #[test]
    fn typescript_breaks_before_the_arrow_of_a_function_type() {
        let code = "Array<T>.reduce<U>(callbackfn: (previous: U, current: T, index: number) => U, initial: U): U";
        assert_eq!(
            wrap_code(code, 60),
            "Array<T>.reduce<U>(\n    callbackfn: (previous: U, current: T, index: number)\n    => U, initial: U): U"
        );
    }

    #[test]
    fn cpp_keeps_the_indent_of_a_broken_line() {
        let code = "  template <typename T> std::vector<T> make(std::size_t count, const T &value)";
        assert_eq!(
            wrap_code(code, 50),
            "  template <typename T> std::vector<T> make(\n      std::size_t count, const T &value)"
        );
    }

    #[test]
    fn a_part_with_nowhere_to_break_stays_long() {
        let code = "very::long::path::to::Something<WithGenerics>";
        assert_eq!(wrap_code(code, 10), code);
    }
}
