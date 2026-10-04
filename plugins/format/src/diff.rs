use std::ops::Range;

use imara_diff::{Algorithm, Diff, InternedInput};

/// Changed lines longer than this are replaced whole instead of character by character.
const MAX_REFINED_BYTES: usize = 64 << 10;

/// The smallest replacements that turn `old` into `new`, in offsets of `old`: lines are
/// compared first, then the characters of the lines that changed, so point and the text
/// around it stay where they were, as with Emacs `replace-buffer-contents`.
pub fn edits(old: &str, new: &str) -> Vec<(Range<usize>, String)> {
    let input = InternedInput::new(old, new);
    let old_lines = starts(input.before.iter().map(|&line| input.interner[line].len()));
    let new_lines = starts(input.after.iter().map(|&line| input.interner[line].len()));
    let mut edits = Vec::new();
    for hunk in Diff::compute(Algorithm::Histogram, &input).hunks() {
        let old_range = old_lines[hunk.before.start as usize]..old_lines[hunk.before.end as usize];
        let new_range = new_lines[hunk.after.start as usize]..new_lines[hunk.after.end as usize];
        if old_range.len().max(new_range.len()) > MAX_REFINED_BYTES {
            edits.push((old_range, new[new_range].to_owned()));
            continue;
        }
        let refined = char_edits(&old[old_range.clone()], &new[new_range]);
        edits.extend(refined.into_iter().map(|(range, text)| {
            (
                old_range.start + range.start..old_range.start + range.end,
                text,
            )
        }));
    }
    edits
}

fn char_edits(old: &str, new: &str) -> Vec<(Range<usize>, String)> {
    let mut input = InternedInput::default();
    input.update_before(old.chars());
    input.update_after(new.chars());
    let old_chars = starts(old.chars().map(char::len_utf8));
    let new_chars = starts(new.chars().map(char::len_utf8));
    Diff::compute(Algorithm::Histogram, &input)
        .hunks()
        .map(|hunk| {
            let old_range =
                old_chars[hunk.before.start as usize]..old_chars[hunk.before.end as usize];
            let new_range =
                new_chars[hunk.after.start as usize]..new_chars[hunk.after.end as usize];
            (old_range, new[new_range].to_owned())
        })
        .collect()
}

/// Byte offset of each token, and of the end.
fn starts(lengths: impl Iterator<Item = usize>) -> Vec<usize> {
    let mut offset = 0;
    let mut starts = vec![0];
    starts.extend(lengths.map(|len| {
        offset += len;
        offset
    }));
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &str, edits: &[(Range<usize>, String)]) -> String {
        let mut text = old.to_owned();
        for (range, inserted) in edits.iter().rev() {
            text.replace_range(range.clone(), inserted);
        }
        text
    }

    #[test]
    fn edits_turn_old_into_new() {
        let cases = [
            ("", "fn main() {}\n"),
            ("a\nb\nc\n", "a\nc\n"),
            ("let x=1;\nlet y =2\n", "let x = 1;\nlet y = 2;\n"),
            ("const é = «a»;", "const é = \"a\";\n"),
            ("same\n", "same\n"),
        ];
        for (old, new) in cases {
            assert_eq!(apply(old, &edits(old, new)), new, "{old:?} → {new:?}");
        }
    }

    #[test]
    fn only_the_changed_characters_are_replaced() {
        let old = "fn main() {\n    call(a,b);\n}\n";
        let new = "fn main() {\n    call(a, b);\n}\n";
        assert_eq!(edits(old, new), [(23..23, " ".to_owned())]);
    }
}
