//! The text between the columns of point and mark on the lines between them, like Emacs
//! rectangles: killed, copied, yanked, deleted, opened or replaced with a string.

use std::ops::{Range, RangeInclusive};

use remacs_core::{Buffer, CommandResult, Completions, Editor, Prompt};

use crate::editing::writable_buffer;

const NO_MARK: &str = "The mark is not set now, so there is no rectangle";

/// The last rectangle killed or copied, like Emacs `killed-rectangle`.
#[derive(Default)]
struct KilledRectangle(Vec<String>);

/// Lines and character columns, as [`Buffer::rectangle`] has them.
struct Rectangle {
    lines: RangeInclusive<usize>,
    columns: Range<usize>,
}

impl Rectangle {
    fn of(buffer: &Buffer) -> Option<Self> {
        let (lines, columns) = buffer.rectangle()?;
        Some(Self { lines, columns })
    }

    /// Each line's bytes inside the columns, and how many columns the line falls short of
    /// where they start.
    fn spans(&self, buffer: &Buffer) -> Vec<(Range<usize>, usize)> {
        let text = buffer.text();
        self.lines
            .clone()
            .map(|line| {
                let line_end = text.line_range(line).map_or(text.len(), |range| range.end);
                let short = self.columns.start.saturating_sub(text.char_column(line_end));
                let start = text.offset_at_char_column(line, self.columns.start);
                (start..text.offset_at_char_column(line, self.columns.end), short)
            })
            .collect()
    }

    /// The text of each line, padded with spaces to the rectangle's width.
    fn extract(&self, buffer: &Buffer) -> Vec<String> {
        self.spans(buffer)
            .into_iter()
            .map(|(span, _)| {
                let text = buffer.contents(span);
                let width = self.columns.len();
                let padding = width.saturating_sub(text.chars().count());
                text + &" ".repeat(padding)
            })
            .collect()
    }
}

fn rectangle(buffer: &Buffer) -> Result<Rectangle, &'static str> {
    Rectangle::of(buffer).ok_or(NO_MARK)
}

/// Turns the region into a rectangle, or back; sets the mark first when it is inactive,
/// like Emacs `rectangle-mark-mode`.
pub fn rectangle_mark_mode(editor: &mut Editor) -> CommandResult {
    let buffer = editor.buffer_mut();
    if buffer.is_rectangle() {
        buffer.set_rectangle(false);
        editor.set_message("Rectangle mark mode disabled");
        return Ok(());
    }
    if !buffer.is_mark_active() {
        buffer.push_mark();
        buffer.activate_mark();
    }
    buffer.set_rectangle(true);
    editor.set_message("Rectangle mark mode enabled");
    Ok(())
}

pub fn kill_rectangle(editor: &mut Editor) -> CommandResult {
    writable_buffer(editor)?;
    save_rectangle(editor)?;
    delete_rectangle(editor)
}

pub fn copy_rectangle_as_kill(editor: &mut Editor) -> CommandResult {
    save_rectangle(editor)?;
    editor.buffer_mut().deactivate_mark();
    Ok(())
}

/// Keeps the rectangle for `yank-rectangle`, and its lines as a kill, for the clipboard.
fn save_rectangle(editor: &mut Editor) -> CommandResult {
    let lines = rectangle(editor.buffer())?.extract(editor.buffer());
    editor.kill_ring.push(lines.join("\n"));
    editor.plugin_state::<KilledRectangle>().0 = lines;
    Ok(())
}

pub fn delete_rectangle(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let rectangle = rectangle(buffer)?;
    let edits =
        rectangle.spans(buffer).into_iter().map(|(span, _)| (span, String::new())).collect();
    finish(buffer, &rectangle, edits);
    Ok(())
}

/// Inserts spaces over the rectangle, moving the text in it to the right.
pub fn open_rectangle(editor: &mut Editor) -> CommandResult {
    let buffer = writable_buffer(editor)?;
    let rectangle = rectangle(buffer)?;
    let width = rectangle.columns.len();
    let edits = rectangle
        .spans(buffer)
        .into_iter()
        .map(|(span, short)| (span.start..span.start, " ".repeat(short + width)))
        .collect();
    finish(buffer, &rectangle, edits);
    Ok(())
}

/// Reads a string and puts it in place of each line of the rectangle.
pub fn string_rectangle(editor: &mut Editor) -> CommandResult {
    rectangle(writable_buffer(editor)?)?;
    let prompt = Prompt {
        label: "String rectangle".into(),
        category: "string-rectangle",
        placeholder: "text",
        hints: &[("↵", "Replace"), ("C-g", "Cancel")],
        ..Prompt::default()
    };
    editor.read(
        prompt,
        |_, _| Completions::default(),
        |editor, string| {
            let buffer = writable_buffer(editor)?;
            let rectangle = rectangle(buffer)?;
            let edits = rectangle
                .spans(buffer)
                .into_iter()
                .map(|(span, short)| (span, " ".repeat(short) + &string))
                .collect();
            finish(buffer, &rectangle, edits);
            Ok(())
        },
    )
}

/// Inserts the last rectangle killed with its top left corner at point, on the lines
/// below as they come, adding lines at the end of the buffer as needed.
pub fn yank_rectangle(editor: &mut Editor) -> CommandResult {
    let lines = editor.plugin_state::<KilledRectangle>().0.clone();
    if lines.is_empty() {
        return Err("No rectangle killed yet".into());
    }
    insert_rectangle(writable_buffer(editor)?, &lines);
    Ok(())
}

/// Whether the newest kill is the last rectangle killed, which `yank` then inserts as one.
pub(crate) fn newest_kill_is_rectangle(editor: &mut Editor) -> Option<Vec<String>> {
    let lines = &editor.plugin_state::<KilledRectangle>().0;
    let joined = lines.join("\n");
    let lines = lines.clone();
    (!lines.is_empty() && editor.kill_ring.newest() == Some(&joined)).then_some(lines)
}

pub(crate) fn insert_rectangle(buffer: &mut Buffer, lines: &[String]) {
    let text = buffer.text();
    let (first_line, column) = (buffer.point_line(), buffer.point_column());
    let mut edits = Vec::new();
    let mut appended = String::new();
    for (index, inserted) in lines.iter().enumerate() {
        let line = first_line + index;
        match text.line_range(line) {
            Some(range) => {
                let short = column.saturating_sub(text.char_column(range.end));
                let at = text.offset_at_char_column(line, column);
                edits.push((at..at, " ".repeat(short) + inserted));
            }
            None => appended += &format!("\n{}{inserted}", " ".repeat(column)),
        }
    }
    let end = text.len();
    let last_line = first_line + lines.len() - 1;
    let last_width = lines.last().map_or(0, |line| line.chars().count());
    if !appended.is_empty() {
        edits.push((end..end, appended));
    }
    let start = buffer.point();
    buffer.apply_edits(edits);
    buffer.set_mark(start);
    let corner = buffer.text().offset_at_char_column(last_line, column + last_width);
    buffer.set_point(corner);
}

/// Makes `edits` and leaves point at the rectangle's top left corner.
fn finish(buffer: &mut Buffer, rectangle: &Rectangle, edits: Vec<(Range<usize>, String)>) {
    buffer.apply_edits(edits);
    let corner =
        buffer.text().offset_at_char_column(*rectangle.lines.start(), rectangle.columns.start);
    buffer.set_point(corner);
    buffer.deactivate_mark();
}
