use std::ops::Range;

use remacs_core::{
    Buffer, CommandResult, ConsoleColor, ConsoleStyle, EchoMessage, Editor, Face, MessageKind,
    MessageLog, Span, TextControls,
};
use remacs_text::Text;

pub const MESSAGES_BUFFER: &str = "*Messages*";
pub const VIEW: &str = "view-echo-area-messages";
pub const CLEAR: &str = "messages-clear";
pub const QUIT: &str = "messages-quit";

const MODE: &str = "Messages";
const HINTS: &[(&str, &str)] = &[("q", "Close"), ("C-c C-k", "Clear")];
/// Longer command names are cut, so the messages stay in one column.
const MAX_COMMAND_WIDTH: usize = 24;
/// Standing for the command of a message that came from background work.
const NO_COMMAND: &str = "—";
const COLUMN_GAP: &str = "  ";
/// The theme's red among the 16 ANSI colors.
const ERROR_COLOR: ConsoleColor = ConsoleColor::Indexed(1);

/// What `*Messages*` was last drawn from, and how.
struct Drawn {
    revision: u64,
    /// Sorted, for the decorator.
    faces: Vec<Span>,
    /// Where the newest message starts; point there follows newer ones.
    last_start: usize,
}

pub fn view(editor: &mut Editor) -> CommandResult {
    if !editor.switch_to_buffer(MESSAGES_BUFFER) {
        editor.add_buffer(Buffer::new(MESSAGES_BUFFER, Text::default(), MODE).read_only());
    }
    update(editor);
    let buffer = editor.window_buffer_mut();
    let last_start = buffer.local::<Drawn>().map_or(0, |drawn| drawn.last_start);
    buffer.set_point(last_start);
    Ok(())
}

pub fn clear(editor: &mut Editor) -> CommandResult {
    editor.message_log_mut().clear();
    Ok(())
}

/// Shows the buffer shown before, like Emacs `quit-window`.
pub fn quit(editor: &mut Editor) -> CommandResult {
    let other = editor
        .buffers()
        .get(1)
        .map(|buffer| buffer.name().to_owned())
        .ok_or("No other buffer")?;
    editor.switch_to_buffer(&other);
    Ok(())
}

pub fn is_shown(editor: &Editor) -> bool {
    editor.minibuffer().is_none() && editor.window_buffer().name() == MESSAGES_BUFFER
}

/// Draws `*Messages*` again when the log changed, if the buffer exists.
pub fn update(editor: &mut Editor) {
    let revision = editor.message_log().revision();
    let drawn_revision = editor
        .buffers()
        .iter()
        .find(|buffer| buffer.name() == MESSAGES_BUFFER)
        .map(|buffer| buffer.local::<Drawn>().map(|drawn| drawn.revision));
    if drawn_revision.is_none_or(|drawn| drawn == Some(revision)) {
        return;
    }
    let (text, drawn) = render(editor.message_log());
    let Some(buffer) = editor.buffer_named_mut(MESSAGES_BUFFER) else {
        return;
    };
    let following = buffer
        .local::<Drawn>()
        .is_none_or(|old| buffer.point() >= old.last_start);
    let last_start = drawn.last_start;
    buffer.replace_text(Text::from(text.as_str()));
    buffer.set_local(drawn);
    if following {
        buffer.set_point(last_start);
    }
}

pub fn decorate(buffer: &Buffer, range: Range<usize>) -> Vec<Span> {
    let Some(drawn) = buffer.local::<Drawn>() else {
        return Vec::new();
    };
    let first = drawn
        .faces
        .partition_point(|span| span.range.end <= range.start);
    drawn.faces[first..]
        .iter()
        .take_while(|span| span.range.start < range.end)
        .cloned()
        .collect()
}

pub fn controls(editor: &Editor) -> Option<TextControls> {
    is_shown(editor).then(|| TextControls {
        hints: HINTS,
        ..TextControls::default()
    })
}

/// One message per entry: the time, the command that showed it and its text, later lines
/// under the first.
fn render(log: &MessageLog) -> (String, Drawn) {
    let command_width = log
        .messages()
        .iter()
        .map(|message| command_name(message).chars().count())
        .max()
        .unwrap_or(0);
    let mut text = String::new();
    let mut faces = Vec::new();
    let mut last_start = 0;
    for message in log.messages() {
        last_start = text.len();
        let command = command_name(message);
        let prefix = format!(
            "{}{COLUMN_GAP}{command:<command_width$}{COLUMN_GAP}",
            message.clock()
        );
        faces.push(Span {
            range: text.len()..text.len() + prefix.trim_end().len(),
            face: Face::Comment,
        });
        text.push_str(&prefix);
        let indent = " ".repeat(prefix.chars().count());
        let body_start = text.len();
        for (index, line) in message.text.lines().enumerate() {
            if index > 0 {
                text.push('\n');
                text.push_str(&indent);
            }
            text.push_str(line);
        }
        if message.kind == MessageKind::Error {
            faces.push(Span {
                range: body_start..text.len(),
                face: Face::Console(ConsoleStyle {
                    color: Some(ERROR_COLOR),
                    ..ConsoleStyle::default()
                }),
            });
        }
        if message.repeats > 0 {
            let count = format!("{COLUMN_GAP}×{}", message.repeats + 1);
            faces.push(Span {
                range: text.len() + COLUMN_GAP.len()..text.len() + count.len(),
                face: Face::Comment,
            });
            text.push_str(&count);
        }
        text.push('\n');
    }
    let drawn = Drawn {
        revision: log.revision(),
        faces,
        last_start,
    };
    (text, drawn)
}

fn command_name(message: &EchoMessage) -> String {
    let name = message.command.unwrap_or(NO_COMMAND);
    if name.chars().count() <= MAX_COMMAND_WIDTH {
        return name.to_owned();
    }
    name.chars()
        .take(MAX_COMMAND_WIDTH - 1)
        .chain(['…'])
        .collect()
}
