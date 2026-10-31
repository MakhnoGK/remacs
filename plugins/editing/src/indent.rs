use std::ops::Range;

use remacs_core::{Buffer, CommandResult, Editor, IndentUnit};

use crate::EditingSettings;
use crate::editing::writable_buffer;
use crate::editorconfig;
use crate::lines::lines_of;

const OPENERS: &[u8] = b"{([";
const CLOSERS: &[u8] = b"})]";
/// Bounds every scan, so files measured in gigabytes stay instant.
const MAX_SCAN: usize = 64 << 10;

/// Reads the unit from the start of `text`; `None` while nothing there is indented.
fn detect(text: &[u8]) -> Option<IndentUnit> {
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

/// What `.editorconfig` files tell of a buffer's indentation, read once.
struct EditorConfigIndent(Option<IndentUnit>);

/// The unit of buffer `index` of [`Editor::buffers`].
pub fn buffer_indent_unit(editor: &mut Editor, index: usize) -> IndentUnit {
    let fallback = fallback_unit(editor, editor.buffers()[index].mode());
    unit_of(&mut editor.buffers_mut()[index], fallback)
}

fn indent_unit(editor: &mut Editor) -> IndentUnit {
    let fallback = fallback_unit(editor, editor.buffer().mode());
    unit_of(editor.buffer_mut(), fallback)
}

/// The buffer's unit as its text shows it, detected once it does; else as `.editorconfig`
/// tells; else `fallback`.
fn unit_of(buffer: &mut Buffer, fallback: IndentUnit) -> IndentUnit {
    if let Some(&unit) = buffer.local::<IndentUnit>() {
        return unit;
    }
    let sample = buffer.text().bytes_in(0..buffer.text().len().min(MAX_SCAN));
    if let Some(unit) = detect(&sample) {
        return *buffer.set_local(unit);
    }
    let told = match buffer.local::<EditorConfigIndent>() {
        Some(told) => told.0,
        None => {
            let told = buffer.path().and_then(editorconfig::indent_unit);
            buffer.set_local(EditorConfigIndent(told)).0
        }
    };
    told.unwrap_or(fallback)
}

/// The unit of a file of `mode` that nothing else tells: editing.toml's for the mode, the
/// mode's own, else editing.toml's.
fn fallback_unit(editor: &mut Editor, mode: &str) -> IndentUnit {
    let modes = editor.mode(mode).and_then(|mode| mode.indent);
    let settings = editor.plugin_state::<EditingSettings>();
    settings.indent_by_mode.get(mode).copied().or(modes).unwrap_or(settings.indent)
}

/// Inserts `text` at point as completions and snippets are written, for any buffer: its
/// lines after the first at the level of point's line, their leading tabs in `unit`. Point
/// goes to `cursor`, an offset in `text`, when there is one.
pub fn insert_indented(buffer: &mut Buffer, unit: IndentUnit, text: &str, cursor: Option<usize>) {
    let start = buffer.point();
    let level = line_indentation(buffer, start);
    let indented = |text: &str| {
        let mut lines = text.split('\n');
        let first = lines.next().unwrap_or_default().to_owned();
        lines.fold(first, |indented, line| {
            let tabs = line.len() - line.trim_start_matches('\t').len();
            format!("{indented}\n{level}{}{}", unit.text().repeat(tabs), &line[tabs..])
        })
    };
    buffer.insert(&indented(text));
    if let Some(before_cursor) = cursor.and_then(|cursor| text.get(..cursor)) {
        buffer.set_point(start + indented(before_cursor).len());
    }
}

/// Breaks the line, indenting the new one: a level deeper after an unclosed bracket, at
/// its opener's level before a closer. Between an empty pair the closer gets a line of its
/// own. Blanks around point go, so no line is left with trailing whitespace.
pub fn newline(editor: &mut Editor) -> CommandResult {
    let left = writable_buffer(editor)?.point_line();
    let unit = indent_unit(editor);
    // The line left first, as Emacs `electric-indent-mode` does: a `}` or `end` typed at
    // the wrong level goes to its block's, and the brackets read it there.
    reindent(editor, left..left + 1, unit);
    let buffer = editor.buffer_mut();
    let point = buffer.point();
    let line = buffer.text().line_range(buffer.point_line()).unwrap_or(point..point);
    let before = buffer.text().bytes_in(line.start.max(point.saturating_sub(MAX_SCAN))..point);
    let after = buffer.text().bytes_in(point..line.end.min(point + MAX_SCAN));
    let blanks = point - trailing_blanks(&before)..point + leading_blanks(&after);
    let opens_block = has_unclosed_opener(&before);
    let ends_with_opener = before[..before.len() - (point - blanks.start)]
        .last()
        .is_some_and(|byte| OPENERS.contains(byte));
    let before_closer =
        after[blanks.end - point..].first().is_some_and(|byte| CLOSERS.contains(byte));

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
    let new_line = buffer.point_line();
    let closer_line = closer_line.map(|closer_line| {
        let resume = buffer.point();
        buffer.insert(&format!("\n{closer_line}"));
        buffer.set_point(resume);
        new_line + 1
    });
    reindent(editor, new_line..closer_line.unwrap_or(new_line) + 1, unit);
    Ok(())
}

/// Indents the line as its syntax tells, or the region's lines; without a syntax that
/// can tell, inserts blanks to the next tab stop. Point in the indentation goes to its end.
pub fn indent_for_tab_command(editor: &mut Editor) -> CommandResult {
    if editor.buffer().region().is_some() {
        return indent_region(editor);
    }
    let line = writable_buffer(editor)?.point_line();
    let unit = indent_unit(editor);
    if !reindent(editor, line..line + 1, unit) {
        return tab_to_tab_stop(editor);
    }
    Ok(())
}

/// Indents the region's lines as their syntax tells; lines it cannot tell keep theirs.
pub fn indent_region(editor: &mut Editor) -> CommandResult {
    let span = writable_buffer(editor)?.region().ok_or("No region to indent")?;
    let unit = indent_unit(editor);
    let (first, last) = lines_of(editor.buffer(), span);
    if !reindent(editor, first..last + 1, unit) {
        return Err("No syntax tells this buffer's indentation".into());
    }
    Ok(())
}

/// Gives `lines` the indentation their syntax tells, one after another, so an earlier
/// line's new indentation carries over to those anchored to it. Lines it cannot tell keep
/// theirs; returns whether it told any. Point in a reindented line's blanks goes to their end.
fn reindent(editor: &mut Editor, lines: Range<usize>, unit: IndentUnit) -> bool {
    let buffer = editor.buffer();
    let mut indented: Vec<Option<String>> = Vec::with_capacity(lines.len());
    for line in lines.clone() {
        let indentation = editor.indentation_of(buffer, line).map(|told| {
            let base = match told.anchor {
                Some(anchor) if lines.contains(&anchor) && anchor < line => indented
                    [anchor - lines.start]
                    .clone()
                    .unwrap_or_else(|| indentation_of_line(buffer, anchor)),
                Some(anchor) => indentation_of_line(buffer, anchor),
                None => String::new(),
            };
            let deeper = if told.deeper { unit.text() } else { String::new() };
            base + &deeper + &" ".repeat(told.align)
        });
        indented.push(indentation);
    }
    if indented.iter().all(Option::is_none) {
        return false;
    }
    let point = buffer.point();
    let mut resume = None;
    let mut edits = Vec::new();
    for (line, indentation) in lines.zip(indented) {
        let Some(indentation) = indentation else {
            continue;
        };
        let range = buffer.text().line_range(line).unwrap_or(0..0);
        let start = range.start;
        let blanks = start..start + indentation_of_line(buffer, line).len();
        let point_in_blanks = (blanks.start..=blanks.end).contains(&point);
        // Blank lines stay empty, but for the one being typed on.
        if blanks.end == range.end && !point_in_blanks {
            continue;
        }
        if point_in_blanks {
            resume = Some((line, indentation.len()));
        }
        if buffer.text().bytes_in(blanks.clone()) != indentation.as_bytes() {
            edits.push((blanks, indentation));
        }
    }
    let buffer = editor.buffer_mut();
    buffer.apply_edits(edits);
    // Found once the lines above have their new blanks.
    if let Some((line, blanks)) = resume {
        let start = buffer.text().line_start(line).unwrap_or(0);
        buffer.set_point(start + blanks);
    }
    true
}

fn indentation_of_line(buffer: &Buffer, line: usize) -> String {
    line_indentation(buffer, buffer.text().line_start(line).unwrap_or(0))
}

/// Starts an indented line below the current one, leaving the current one whole.
pub fn open_line_below(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let line_end =
        buffer.text().line_range(buffer.point_line()).map_or(buffer.point(), |line| line.end);
    buffer.set_point(line_end);
    newline(editor)
}

/// Starts a line above the current one at its level, a level deeper above a closer.
pub fn open_line_above(editor: &mut Editor) -> CommandResult {
    writable_buffer(editor)?;
    let unit = indent_unit(editor);
    let buffer = editor.buffer_mut();
    let line = buffer.text().line_range(buffer.point_line()).unwrap_or(0..0);
    let mut indentation = line_indentation(buffer, line.start);
    let first =
        buffer.text().bytes_in(line.start + indentation.len()..line.end.min(line.start + MAX_SCAN));
    if first.first().is_some_and(|byte| CLOSERS.contains(byte)) {
        indentation.push_str(&unit.text());
    }
    buffer.set_point(line.start);
    buffer.insert(&indentation);
    let resume = buffer.point();
    buffer.insert("\n");
    buffer.set_point(resume);
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
    writable_buffer(editor)?;
    let text = match indent_unit(editor) {
        IndentUnit::Tabs => "\t".into(),
        IndentUnit::Spaces(width) => " ".repeat(width - editor.buffer().point_column() % width),
    };
    editor.buffer_mut().insert(&text);
    Ok(())
}

/// The blanks starting the line that holds `offset`.
fn line_indentation(buffer: &Buffer, offset: usize) -> String {
    let text = buffer.text();
    let line = text.line_range(text.line_of(offset)).unwrap_or(offset..offset);
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
    use remacs_core::IndentUnit;

    use super::detect;

    #[test]
    fn detects_the_unit_a_text_uses() {
        let two = b"fn main() {\n  if x {\n    y();\n  }\n}\n";
        assert_eq!(detect(two), Some(IndentUnit::Spaces(2)));
        let tabs = b"int main() {\n\tif (x) {\n\t\ty();\n\t}\n}\n";
        assert_eq!(detect(tabs), Some(IndentUnit::Tabs));
        let doc_comments = b"/**\n * Docs\n */\nfn f() {\n    g();\n}\n";
        assert_eq!(detect(doc_comments), Some(IndentUnit::Spaces(4)));
        assert_eq!(detect(b"flat\ntext\n"), None);
    }
}
