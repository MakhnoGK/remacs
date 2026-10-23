use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use ::regex::bytes::{Regex, RegexBuilder};
use memchr::memmem::Finder;
use remacs_core::{Completions, PromptOption};
use remacs_text::Snapshot;

/// Bytes copied into the search window at a time; small enough to stay in cache.
const SEARCHED_BLOCK: usize = 64 * 1024;
/// Bytes after its start a regular expression's match is looked for in at least; a longer
/// match may be found in pieces.
const MAX_MATCH: usize = 4 * 1024;

/// What to find: a literal string, or a regular expression; either with smart case like
/// the prompts' filters: case is ignored unless it has an uppercase letter.
#[derive(Clone)]
pub enum Query {
    Literal {
        finder: Finder<'static>,
        ignore_case: bool,
        len: usize,
    },
    Regex(Regex),
}

/// The switch, `M-r`, between literal text and a regular expression in a search prompt.
pub fn regexp_option(regex: &Rc<Cell<bool>>) -> PromptOption {
    PromptOption::chip('r', "Regexp", regex)
}

/// `input` as a regular expression, its `^` and `$` at line ends, case ignored unless it
/// has an uppercase letter outside an escape like `\W`; why not, if it is invalid.
pub fn regex(input: &str) -> Result<Regex, String> {
    let mut escaped = false;
    let has_uppercase = input.chars().any(|character| {
        let uppercase = !escaped && character.is_uppercase();
        escaped = !escaped && character == '\\';
        uppercase
    });
    RegexBuilder::new(input)
        .case_insensitive(!has_uppercase)
        .multi_line(true)
        // Project files are searched as they are on disk, maybe with CRLF line ends.
        .crlf(true)
        .build()
        .map_err(|error| {
            // A syntax error ends with its message, after the pattern drawn with a caret.
            let error = error.to_string();
            let reason = error.lines().last().unwrap_or_default();
            format!("Invalid regexp: {}", reason.trim_start_matches("error: "))
        })
}

/// What a prompt lists in place of candidates: only `text`, e.g. why its input is invalid.
pub fn note(text: String) -> Completions {
    Completions {
        note: Some(text),
        ..Completions::default()
    }
}

/// `line` without the spaces around it, and `ranges` of it moved to match.
pub fn trimmed(line: &str, ranges: Vec<Range<usize>>) -> (String, Vec<Range<usize>>) {
    let label = line.trim();
    let offset = line.len() - line.trim_start().len();
    let ranges = ranges
        .into_iter()
        .map(|range| {
            range.start.saturating_sub(offset)..range.end.saturating_sub(offset).min(label.len())
        })
        .filter(|range| !range.is_empty())
        .collect();
    (label.to_owned(), ranges)
}

impl Query {
    /// `text` as a literal string, or with `regex`, a regular expression; none when empty.
    pub fn new(text: &str, regex: bool) -> Result<Option<Self>, String> {
        if text.is_empty() {
            return Ok(None);
        }
        if regex {
            return self::regex(text).map(|regex| Some(Self::Regex(regex)));
        }
        let ignore_case = !text.chars().any(char::is_uppercase);
        let needle = if ignore_case {
            text.to_ascii_lowercase()
        } else {
            text.to_owned()
        };
        Ok(Some(Self::Literal {
            finder: Finder::new(needle.as_bytes()).into_owned(),
            ignore_case,
            len: text.len(),
        }))
    }

    /// Like [`Query::new`], with what the prompt lists instead when there is nothing to
    /// find: no candidates, or why the regular expression is invalid.
    pub fn parse(text: &str, regex: bool) -> Result<Self, Box<Completions>> {
        match Self::new(text, regex) {
            Ok(Some(query)) => Ok(query),
            Ok(None) => Err(Box::default()),
            Err(error) => Err(Box::new(note(error))),
        }
    }

    /// Calls `each` with every match that starts within `range` of `text`, or an empty one
    /// at its end, in order, until it returns false. Reads a block at a time, so a file of
    /// gigabytes, which an unedited buffer holds as one piece, is never copied whole.
    pub fn find(
        &self,
        text: &Snapshot,
        range: Range<usize>,
        each: impl FnMut(Range<usize>) -> bool,
    ) {
        match self {
            Self::Literal {
                finder,
                ignore_case,
                len,
            } => find_literal(finder, *ignore_case, *len, text, range, each),
            Self::Regex(regex) => find_regex(regex, text, range, each),
        }
    }

    /// Byte ranges of the matches in `line`, empty ones left out.
    pub fn ranges_in(&self, line: &str) -> Vec<Range<usize>> {
        match self {
            Self::Literal {
                finder,
                ignore_case,
                len,
            } => {
                let haystack = if *ignore_case {
                    line.to_ascii_lowercase()
                } else {
                    line.to_owned()
                };
                finder
                    .find_iter(haystack.as_bytes())
                    .map(|start| start..start + len)
                    .collect()
            }
            Self::Regex(regex) => regex
                .find_iter(line.as_bytes())
                .map(|found| found.range())
                .filter(|range| !range.is_empty())
                .collect(),
        }
    }

    /// What replaces the match at `range` of `text`: `to`, or for a regular expression,
    /// `to` with `$1`, `${name}` and `$0` standing for its groups and the whole match.
    pub fn replacement(&self, text: &Snapshot, range: Range<usize>, to: &str) -> String {
        let Self::Regex(regex) = self else {
            return to.to_owned();
        };
        let from = text.previous_char_boundary(range.start);
        let found = range.start - from..range.end - from;
        // A character on each side is all `^`, `$` and `\b` see, so the match is copied
        // alone. Unless the character after it lets the regexp match further, as `a+` does
        // past a match cut at the end of what was searched, or `ab$|a` in `abc`; then the
        // match is cut where it ended, as it was.
        for end in [text.next_char_boundary(range.end), range.end] {
            let haystack = text.bytes_in(from..end);
            if let Some(captures) = regex.captures_at(&haystack, found.start)
                && captures.get(0).map(|whole| whole.range()) == Some(found.clone())
            {
                let mut replacement = Vec::new();
                captures.expand(to.as_bytes(), &mut replacement);
                return String::from_utf8_lossy(&replacement).into_owned();
            }
        }
        // Never seen; the match is left as it is rather than lost.
        String::from_utf8_lossy(&text.bytes_in(range)).into_owned()
    }

    /// Why `to` cannot replace this regular expression's matches: a group it names that
    /// the regexp does not have, as `$1_new` names `1_new`, the letters, digits and `_`
    /// after a `$` being one name.
    pub fn check_replacement(&self, to: &str) -> Result<(), String> {
        let Self::Regex(regex) = self else {
            return Ok(());
        };
        let mut rest = to;
        while let Some(dollar) = rest.find('$') {
            rest = &rest[dollar + 1..];
            // `$$` is a dollar.
            if let Some(after) = rest.strip_prefix('$') {
                rest = after;
                continue;
            }
            let (name, braced) = match rest.strip_prefix('{') {
                Some(inside) => match inside.find('}') {
                    Some(close) => (&inside[..close], true),
                    None => continue,
                },
                None => {
                    let len = rest
                        .find(|character: char| {
                            !character.is_ascii_alphanumeric() && character != '_'
                        })
                        .unwrap_or(rest.len());
                    (&rest[..len], false)
                }
            };
            rest = &rest[name.len() + if braced { 2 } else { 0 }..];
            let known = match name.parse::<usize>() {
                Ok(index) => index < regex.captures_len(),
                Err(_) => regex.capture_names().flatten().any(|group| group == name),
            };
            if !name.is_empty() && !known {
                let digits =
                    name.len() - name.trim_start_matches(|c: char| c.is_ascii_digit()).len();
                let hint = match (braced, digits) {
                    (false, 1..) => format!(
                        "; braces end a group's number, as in ${{{}}}{}",
                        &name[..digits],
                        &name[digits..]
                    ),
                    _ => String::new(),
                };
                return Err(format!("No group “{name}” in the regexp{hint}"));
            }
        }
        Ok(())
    }
}

fn find_literal(
    finder: &Finder,
    ignore_case: bool,
    len: usize,
    text: &Snapshot,
    range: Range<usize>,
    mut each: impl FnMut(Range<usize>) -> bool,
) {
    // Enough of the previous block for a match that crosses into the next.
    let overlap = len - 1;
    let mut window = Vec::with_capacity(SEARCHED_BLOCK + overlap);
    let mut window_start = range.start;
    // Matches do not overlap: the next may start only after the last one reported.
    let mut next_allowed = range.start;
    let searched = range.start..(range.end + overlap).min(text.len());
    for block in text
        .chunks(searched)
        .flat_map(|chunk| chunk.chunks(SEARCHED_BLOCK))
    {
        let kept = window.len().min(overlap);
        window_start += window.len() - kept;
        window.drain(..window.len() - kept);
        window.extend_from_slice(block);
        if ignore_case {
            window[kept..].make_ascii_lowercase();
        }
        let mut from = next_allowed.saturating_sub(window_start);
        while let Some(found) = window.get(from..).and_then(|rest| finder.find(rest)) {
            let start = window_start + from + found;
            if start >= range.end {
                return;
            }
            next_allowed = start + len;
            from = next_allowed - window_start;
            if !each(start..next_allowed) {
                return;
            }
        }
    }
}

/// Searches a block at a time from the character before `range`, so that `^` and `\b`
/// see what precedes it, with [`MAX_MATCH`] bytes after the start of each match.
// shortcut: a match longer than MAX_MATCH is found in pieces; raise it if such matches matter.
fn find_regex(
    regex: &Regex,
    text: &Snapshot,
    range: Range<usize>,
    mut each: impl FnMut(Range<usize>) -> bool,
) {
    let from = text.previous_char_boundary(range.start);
    let to = (range.end + MAX_MATCH).min(text.len());
    let mut window = Vec::new();
    let mut window_start = from;
    let mut position = range.start - from;
    // Like Rust's regex and Emacs, no empty match right where the last one ended.
    let mut last_end = None;
    for block in text
        .chunks(from..to)
        .flat_map(|chunk| chunk.chunks(SEARCHED_BLOCK))
    {
        window.extend_from_slice(block);
        let last = window_start + window.len() == to;
        // Small pieces of an edited buffer are gathered, so that the text after the cut,
        // searched again with the next block, is a small part of what is searched.
        if !last && window.len() < position + SEARCHED_BLOCK + MAX_MATCH {
            continue;
        }
        // Matches starting before here have MAX_MATCH bytes to end in.
        let cut = if last {
            window.len() + 1
        } else {
            window.len() - MAX_MATCH
        };
        while position < cut {
            let Some(found) = regex.find_at(&window, position) else {
                break;
            };
            if found.start() >= cut {
                break;
            }
            let matched = window_start + found.start()..window_start + found.end();
            if matched.start > range.end || (matched.start == range.end && !matched.is_empty()) {
                return;
            }
            let after_last = matched.is_empty() && last_end == Some(matched.start);
            if !after_last && !each(matched.clone()) {
                return;
            }
            last_end = Some(matched.end);
            position = match found.is_empty() {
                true => found.end() + char_len(&window, found.end()),
                false => found.end(),
            };
        }
        if last {
            return;
        }
        // The character before where the search goes on stays, as what precedes it.
        position = position.max(cut);
        let mut kept = position.saturating_sub(1);
        while kept > 0 && is_continuation(window[kept]) {
            kept -= 1;
        }
        window.drain(..kept);
        window_start += kept;
        position -= kept;
    }
}

fn is_continuation(byte: u8) -> bool {
    byte & 0xC0 == 0x80
}

/// Bytes of the character starting at `offset`, 1 past the end.
fn char_len(bytes: &[u8], offset: usize) -> usize {
    bytes
        .get(offset)
        .map_or(1, |byte| (byte.leading_ones() as usize).max(1))
}
