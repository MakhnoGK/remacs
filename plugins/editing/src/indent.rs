use remacs_core::{Buffer, CommandResult, Editor};

use crate::editing::writable_buffer;

const OPENERS: &[u8] = b"{([";
const CLOSERS: &[u8] = b"})]";
const DEFAULT_WIDTH: usize = 4;
/// Bounds every scan, so files measured in gigabytes stay instant.
const MAX_SCAN: usize = 64 << 10;

/// What one level of indentation is in a buffer, as its text already uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndentUnit {
    Tabs,
    Spaces(usize),
}

impl IndentUnit {
    fn text(self) -> String {
        match self {
            IndentUnit::Tabs => "\t".into(),
            IndentUnit::Spaces(width) => " ".repeat(width),
        }
    }

    /// Reads the unit from the start of `text`; `None` while nothing there is indented.
    fn detect(text: &[u8]) -> Option<Self> {
        let mut tab_lines = 0;
        let mut space_lines = 0;
        let mut step_counts = [0usize; 9];
        let mut previous_spaces = 0;
        for line in text.split(|&byte| byte == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            if line[0] == b'\t' {
                tab_lines += 1;
                continue;
            }
            let spaces = line.iter().take_while(|&&byte| byte == b' ').count();
            if spaces > 0 {
                space_lines += 1;
            }
            // Steps of one are block-comment continuations (` * `), not indentation.
            let step = spaces.saturating_sub(previous_spaces);
            if (2..step_counts.len()).contains(&step) {
                step_counts[step] += 1;
            }
            previous_spaces = spaces;
        }
        if tab_lines > space_lines {
            return Some(IndentUnit::Tabs);
        }
        (0..step_counts.len())
            .rev()
            .max_by_key(|&step| step_counts[step])
            .filter(|&step| step_counts[step] > 0)
            .map(IndentUnit::Spaces)
    }
}

/// The buffer's unit, detected once its text shows one; the default until then.
pub fn indent_unit(buffer: &mut Buffer) -> IndentUnit {
    if let Some(&unit) = buffer.local::<IndentUnit>() {
        return unit;
    }
    let sample = buffer.text().bytes_in(0..buffer.text().len().min(MAX_SCAN));
    match IndentUnit::detect(&sample) {
        Some(unit) => *buffer.set_local(unit),
        None => IndentUnit::Spaces(DEFAULT_WIDTH),
    }
}

/// Breaks the line, indenting the new one: a level deeper after an unclosed bracket, at
/// its opener's level before a closer. Between an empty pair the closer gets a line of its
/// own. Blanks around point go, so no line is left with trailing whitespace.
pub fn newline(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let unit = indent_unit(buffer);
    let point = buffer.point();
    let line = buffer
        .text()
        .line_range(buffer.point_line())
        .unwrap_or(point..point);
    let before = buffer
        .text()
        .bytes_in(line.start.max(point.saturating_sub(MAX_SCAN))..point);
    let after = buffer
        .text()
        .bytes_in(point..line.end.min(point + MAX_SCAN));
    let blanks = point - trailing_blanks(&before)..point + leading_blanks(&after);
    let opens_block = has_unclosed_opener(&before);
    let ends_with_opener = before[..before.len() - (point - blanks.start)]
        .last()
        .is_some_and(|byte| OPENERS.contains(byte));
    let before_closer = after[blanks.end - point..]
        .first()
        .is_some_and(|byte| CLOSERS.contains(byte));

    let indentation = line_indentation(buffer, point);
    let deeper = || format!("{indentation}{}", unit.text());
    let (new_line, closer_line) = match (before_closer, opens_block) {
        (true, true) if ends_with_opener => (deeper(), Some(indentation.clone())),
        (true, false) => {
            let opener = matching_opener(buffer, blanks.start).unwrap_or(point);
            (line_indentation(buffer, opener), None)
        }
        (_, true) => (deeper(), None),
        (_, false) => (indentation.clone(), None),
    };

    buffer.delete(blanks);
    buffer.insert(&format!("\n{new_line}"));
    if let Some(closer_line) = closer_line {
        let resume = buffer.point();
        buffer.insert(&format!("\n{closer_line}"));
        buffer.set_point(resume);
    }
    Ok(())
}

/// Before a closer is typed on a line holding only blanks, indents the line to the level
/// of the closer's opener.
pub fn indent_for_closer(buffer: &mut Buffer, closer: char) {
    if !u8::try_from(closer).is_ok_and(|byte| CLOSERS.contains(&byte)) {
        return;
    }
    let point = buffer.point();
    let line_start = buffer.text().line_start(buffer.point_line()).unwrap_or(0);
    let head = buffer.text().bytes_in(line_start..point);
    if leading_blanks(&head) < head.len() {
        return;
    }
    let Some(opener) = matching_opener(buffer, line_start) else {
        return;
    };
    let indentation = line_indentation(buffer, opener);
    buffer.delete(line_start..point);
    buffer.insert(&indentation);
}

pub fn tab_to_tab_stop(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let text = match indent_unit(buffer) {
        IndentUnit::Tabs => "\t".into(),
        IndentUnit::Spaces(width) => " ".repeat(width - buffer.point_column() % width),
    };
    buffer.insert(&text);
    Ok(())
}

/// The blanks starting the line that holds `offset`.
fn line_indentation(buffer: &Buffer, offset: usize) -> String {
    let text = buffer.text();
    let line = text
        .line_range(text.line_of(offset))
        .unwrap_or(offset..offset);
    let head = text.bytes_in(line.start..line.end.min(line.start + MAX_SCAN));
    String::from_utf8_lossy(&head[..leading_blanks(&head)]).into_owned()
}

/// Whether some bracket opened in `line` is still open at its end.
fn has_unclosed_opener(line: &[u8]) -> bool {
    let mut depth = 0usize;
    for byte in line {
        if OPENERS.contains(byte) {
            depth += 1;
        } else if CLOSERS.contains(byte) {
            depth = depth.saturating_sub(1);
        }
    }
    depth > 0
}

/// The innermost bracket still open at `offset`, searched back within the scan bound.
fn matching_opener(buffer: &Buffer, offset: usize) -> Option<usize> {
    let start = offset.saturating_sub(MAX_SCAN);
    let bytes = buffer.text().bytes_in(start..offset);
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().rev() {
        if CLOSERS.contains(byte) {
            depth += 1;
        } else if OPENERS.contains(byte) {
            if depth == 0 {
                return Some(start + index);
            }
            depth -= 1;
        }
    }
    None
}

fn is_blank(byte: &&u8) -> bool {
    **byte == b' ' || **byte == b'\t'
}

fn leading_blanks(bytes: &[u8]) -> usize {
    bytes.iter().take_while(is_blank).count()
}

fn trailing_blanks(bytes: &[u8]) -> usize {
    bytes.iter().rev().take_while(is_blank).count()
}

#[cfg(test)]
mod tests {
    use super::IndentUnit;

    #[test]
    fn detects_the_unit_a_text_uses() {
        let two = b"fn main() {\n  if x {\n    y();\n  }\n}\n";
        assert_eq!(IndentUnit::detect(two), Some(IndentUnit::Spaces(2)));
        let tabs = b"int main() {\n\tif (x) {\n\t\ty();\n\t}\n}\n";
        assert_eq!(IndentUnit::detect(tabs), Some(IndentUnit::Tabs));
        let doc_comments = b"/**\n * Docs\n */\nfn f() {\n    g();\n}\n";
        assert_eq!(
            IndentUnit::detect(doc_comments),
            Some(IndentUnit::Spaces(4))
        );
        assert_eq!(IndentUnit::detect(b"flat\ntext\n"), None);
    }
}
